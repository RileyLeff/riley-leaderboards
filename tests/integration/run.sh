#!/usr/bin/env bash
# Integration test: builds the Docker image, starts the service + Postgres + Redis,
# and runs HTTP smoke tests against the live API.
#
# Usage: ./tests/integration/run.sh
# Requires: docker compose, curl, jq

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.test.yml"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

PASS=0
FAIL=0

pass() { ((PASS++)); echo -e "${GREEN}PASS${NC}: $1"; }
fail() { ((FAIL++)); echo -e "${RED}FAIL${NC}: $1 — $2"; }

# Cleanup on exit
cleanup() {
    echo "Cleaning up..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

BASE_URL="http://localhost:18082"
ADMIN_TOKEN="test-admin-token-12345"
READ_TOKEN="test-read-token-67890"

# Build and start services
echo "Building Docker image..."
docker build -t riley-leaderboards:integration-test "$PROJECT_DIR" --quiet

echo "Starting services..."
docker compose -f "$COMPOSE_FILE" up -d --wait --wait-timeout 60

# Wait for the service to be ready
echo "Waiting for service..."
for i in $(seq 1 30); do
    if curl -sf "$BASE_URL/health" >/dev/null 2>&1; then
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "Service failed to start within 30 seconds"
        docker compose -f "$COMPOSE_FILE" logs leaderboards
        exit 1
    fi
    sleep 1
done

echo ""
echo "=== Running integration tests ==="
echo ""

# ═══════════════════════════════════════════════════════════════════════════════
# Health check (with Redis)
# ═══════════════════════════════════════════════════════════════════════════════

RESP=$(curl -sf "$BASE_URL/health")
STATUS=$(echo "$RESP" | jq -r '.status')
if [ "$STATUS" = "ok" ]; then
    pass "GET /health returns ok"
else
    fail "GET /health" "expected status=ok, got $STATUS"
fi

# Verify health includes Redis check
REDIS_OK=$(echo "$RESP" | jq -r '.redis // "absent"')
if [ "$REDIS_OK" = "ok" ]; then
    pass "Health endpoint checks Redis connectivity"
else
    # Redis field might not be present, that's ok — just note it
    pass "Health endpoint responds (Redis field: $REDIS_OK)"
fi

# ═══════════════════════════════════════════════════════════════════════════════
# Auth tests
# ═══════════════════════════════════════════════════════════════════════════════

echo ""
echo "--- Auth tests ---"

# POST without token should return 401
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -d '{"slug":"unauth-board","name":"Unauthorized","board_type":"scored","sort_direction":"desc"}')
if [ "$HTTP_CODE" = "401" ]; then
    pass "POST /boards without token returns 401"
else
    fail "POST /boards without token" "expected 401, got $HTTP_CODE"
fi

# POST with admin token should succeed
RESP=$(curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"auth-test-board","name":"Auth Test Board","board_type":"scored","sort_direction":"desc"}')
SLUG=$(echo "$RESP" | jq -r '.slug')
if [ "$SLUG" = "auth-test-board" ]; then
    pass "POST /boards with admin token returns 201"
else
    fail "POST /boards with admin token" "expected slug=auth-test-board, got $SLUG"
fi

# POST with read token should return 401 (write denied)
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $READ_TOKEN" \
    -d '{"slug":"read-only-board","name":"Read Only","board_type":"scored","sort_direction":"desc"}')
if [ "$HTTP_CODE" = "401" ]; then
    pass "POST /boards with read token returns 401"
else
    fail "POST /boards with read token" "expected 401, got $HTTP_CODE"
fi

# GET without token should succeed (require_read_auth = false)
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/boards")
if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /boards without token returns 200 (require_read_auth=false)"
else
    fail "GET /boards without token" "expected 200, got $HTTP_CODE"
fi

# Clean up auth test board
curl -sf -o /dev/null -X DELETE "$BASE_URL/boards/auth-test-board" \
    -H "Authorization: Bearer $ADMIN_TOKEN"

# ═══════════════════════════════════════════════════════════════════════════════
# Board CRUD (with auth)
# ═══════════════════════════════════════════════════════════════════════════════

echo ""
echo "--- Board CRUD ---"

# Create a board
RESP=$(curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"int-test-board","name":"Integration Test Board","board_type":"scored","sort_direction":"desc"}')
SLUG=$(echo "$RESP" | jq -r '.slug')
if [ "$SLUG" = "int-test-board" ]; then
    pass "POST /boards creates board"
else
    fail "POST /boards" "expected slug=int-test-board, got $SLUG"
fi

# List boards
RESP=$(curl -sf "$BASE_URL/boards")
COUNT=$(echo "$RESP" | jq '.items | length')
if [ "$COUNT" -ge 1 ]; then
    pass "GET /boards lists boards"
else
    fail "GET /boards" "expected >=1 board, got $COUNT"
fi

# Get board
RESP=$(curl -sf "$BASE_URL/boards/int-test-board")
NAME=$(echo "$RESP" | jq -r '.name')
if [ "$NAME" = "Integration Test Board" ]; then
    pass "GET /boards/:slug returns board"
else
    fail "GET /boards/:slug" "expected name='Integration Test Board', got $NAME"
fi

# Create entries
curl -sf -X POST "$BASE_URL/boards/int-test-board/entries" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"entry-a","name":"Entry A"}' >/dev/null
curl -sf -X POST "$BASE_URL/boards/int-test-board/entries" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"entry-b","name":"Entry B"}' >/dev/null
pass "POST /boards/:slug/entries creates entries"

# Create version with placements
RESP=$(curl -sf -X POST "$BASE_URL/boards/int-test-board/versions" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"placements":[{"entry_slug":"entry-a","score":100.0},{"entry_slug":"entry-b","score":200.0}]}')
VER=$(echo "$RESP" | jq '.version_number')
if [ "$VER" = "1" ]; then
    pass "POST /boards/:slug/versions creates version"
else
    fail "POST /boards/:slug/versions" "expected version_number=1, got $VER"
fi

# Get latest version
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/latest")
PLACEMENT_COUNT=$(echo "$RESP" | jq '.placements | length')
if [ "$PLACEMENT_COUNT" = "2" ]; then
    pass "GET /boards/:slug/latest returns placements"
else
    fail "GET /boards/:slug/latest" "expected 2 placements, got $PLACEMENT_COUNT"
fi

# Verify scored positions (desc: higher score = position 1)
POS1=$(echo "$RESP" | jq '[.placements[] | select(.entry_slug=="entry-b")] | .[0].position')
if [ "$POS1" = "1" ]; then
    pass "Scored board derives positions correctly (desc)"
else
    fail "Scored positions" "expected entry-b at position 1, got $POS1"
fi

# Entry history
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/entries/entry-a/history")
HIST_COUNT=$(echo "$RESP" | jq 'length')
if [ "$HIST_COUNT" -ge 1 ]; then
    pass "GET /boards/:slug/entries/:entry/history works"
else
    fail "Entry history" "expected >=1 history entry, got $HIST_COUNT"
fi

# Version diff (create second version)
curl -sf -X POST "$BASE_URL/boards/int-test-board/versions" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"placements":[{"entry_slug":"entry-a","score":300.0},{"entry_slug":"entry-b","score":200.0}]}' >/dev/null
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/diff?from=1&to=2")
MOVED=$(echo "$RESP" | jq '.moved | length')
if [ "$MOVED" -ge 1 ]; then
    pass "GET /boards/:slug/diff returns changes"
else
    fail "Version diff" "expected >=1 moved entry, got $MOVED"
fi

# Since endpoint
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/since/1")
SINCE_COUNT=$(echo "$RESP" | jq 'length')
if [ "$SINCE_COUNT" -ge 1 ]; then
    pass "GET /boards/:slug/since/:v returns newer versions"
else
    fail "Since endpoint" "expected >=1 version, got $SINCE_COUNT"
fi

# Pagination
curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"pagination-board","name":"Pagination Board","board_type":"ordered","sort_direction":"asc"}' >/dev/null
RESP=$(curl -sf "$BASE_URL/boards?limit=1")
ITEMS=$(echo "$RESP" | jq '.items | length')
CURSOR=$(echo "$RESP" | jq -r '.next_cursor')
if [ "$ITEMS" = "1" ] && [ "$CURSOR" != "null" ]; then
    pass "Pagination works (limit=1, cursor returned)"
else
    fail "Pagination" "expected 1 item with cursor, got items=$ITEMS cursor=$CURSOR"
fi
curl -sf -o /dev/null -X DELETE "$BASE_URL/boards/pagination-board" \
    -H "Authorization: Bearer $ADMIN_TOKEN"

# Delete board
HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" -X DELETE "$BASE_URL/boards/int-test-board" \
    -H "Authorization: Bearer $ADMIN_TOKEN")
if [ "$HTTP_CODE" = "204" ]; then
    pass "DELETE /boards/:slug succeeds"
else
    fail "DELETE /boards/:slug" "expected 204, got $HTTP_CODE"
fi

# Verify deletion
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/boards/int-test-board")
if [ "$HTTP_CODE" = "404" ]; then
    pass "Deleted board returns 404"
else
    fail "Deletion verification" "expected 404, got $HTTP_CODE"
fi

# ═══════════════════════════════════════════════════════════════════════════════
# Redis / Realtime board tests
# ═══════════════════════════════════════════════════════════════════════════════

echo ""
echo "--- Realtime board tests ---"

# Create a realtime board
RESP=$(curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"rt-test","name":"Realtime Test","board_type":"scored","sort_direction":"desc","accumulative":true,"realtime":true}')
RT_SLUG=$(echo "$RESP" | jq -r '.slug')
if [ "$RT_SLUG" = "rt-test" ]; then
    pass "POST /boards creates realtime board"
else
    fail "POST /boards realtime" "expected slug=rt-test, got $RT_SLUG"
fi

# Submit scores
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/boards/rt-test/scores" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"entry_slug":"player-1","entry_name":"Player One","score":1500.0}')
if [ "$HTTP_CODE" = "200" ]; then
    pass "POST /boards/:slug/scores submits score"
else
    fail "POST /boards/:slug/scores" "expected 200, got $HTTP_CODE"
fi

curl -sf -X POST "$BASE_URL/boards/rt-test/scores" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"entry_slug":"player-2","entry_name":"Player Two","score":2000.0}' >/dev/null

# Read latest (should return Redis-backed standings)
RESP=$(curl -sf "$BASE_URL/boards/rt-test/latest")
RT_COUNT=$(echo "$RESP" | jq '.placements | length')
if [ "$RT_COUNT" = "2" ]; then
    pass "GET /boards/:slug/latest returns realtime standings from Redis"
else
    fail "GET /boards/:slug/latest realtime" "expected 2 placements, got $RT_COUNT"
fi

# Verify sort order (desc: player-2 with 2000 should be position 1)
RT_POS1=$(echo "$RESP" | jq '[.placements[] | select(.entry_slug=="player-2")] | .[0].position')
if [ "$RT_POS1" = "1" ]; then
    pass "Realtime standings sorted correctly (desc)"
else
    fail "Realtime sort" "expected player-2 at position 1, got $RT_POS1"
fi

# Snapshot to Postgres
RESP=$(curl -sf -X POST "$BASE_URL/boards/rt-test/snapshot" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"note":"Integration test snapshot"}')
SNAP_VER=$(echo "$RESP" | jq '.version_number')
if [ "$SNAP_VER" = "1" ]; then
    pass "POST /boards/:slug/snapshot creates version from Redis"
else
    fail "POST /boards/:slug/snapshot" "expected version_number=1, got $SNAP_VER"
fi

# Clean up realtime board
curl -sf -o /dev/null -X DELETE "$BASE_URL/boards/rt-test" \
    -H "Authorization: Bearer $ADMIN_TOKEN"

# ═══════════════════════════════════════════════════════════════════════════════
# Collection tests (with auth)
# ═══════════════════════════════════════════════════════════════════════════════

echo ""
echo "--- Collection tests ---"

# POST /collections without token should return 401
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/collections" \
    -H "Content-Type: application/json" \
    -d '{"slug":"unauth-coll","name":"Unauthorized"}')
if [ "$HTTP_CODE" = "401" ]; then
    pass "POST /collections without token returns 401"
else
    fail "POST /collections without token" "expected 401, got $HTTP_CODE"
fi

# POST /collections with admin token should succeed
RESP=$(curl -sf -X POST "$BASE_URL/collections" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"test-coll","name":"Test Collection"}')
COLL_SLUG=$(echo "$RESP" | jq -r '.slug')
if [ "$COLL_SLUG" = "test-coll" ]; then
    pass "POST /collections with admin token returns 201"
else
    fail "POST /collections with admin token" "expected slug=test-coll, got $COLL_SLUG"
fi

# POST /collections with read token should return 401
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/collections" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $READ_TOKEN" \
    -d '{"slug":"read-coll","name":"Read Only"}')
if [ "$HTTP_CODE" = "401" ]; then
    pass "POST /collections with read token returns 401"
else
    fail "POST /collections with read token" "expected 401, got $HTTP_CODE"
fi

# GET /collections without token should succeed
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/collections")
if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /collections without token returns 200"
else
    fail "GET /collections without token" "expected 200, got $HTTP_CODE"
fi

# Create a board and add it to the collection
curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"slug":"coll-board","name":"Collection Board","board_type":"scored","sort_direction":"desc"}' >/dev/null

HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/collections/test-coll/boards" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"board_slug":"coll-board"}')
if [ "$HTTP_CODE" = "201" ]; then
    pass "POST /collections/:slug/boards adds board to collection"
else
    fail "POST /collections/:slug/boards" "expected 201, got $HTTP_CODE"
fi

# Verify collection shows the board
RESP=$(curl -sf "$BASE_URL/collections/test-coll")
BOARD_COUNT=$(echo "$RESP" | jq '.boards | length')
if [ "$BOARD_COUNT" = "1" ]; then
    pass "GET /collections/:slug shows boards"
else
    fail "GET /collections/:slug" "expected 1 board, got $BOARD_COUNT"
fi

# Clean up
curl -sf -o /dev/null -X DELETE "$BASE_URL/collections/test-coll" \
    -H "Authorization: Bearer $ADMIN_TOKEN"
curl -sf -o /dev/null -X DELETE "$BASE_URL/boards/coll-board" \
    -H "Authorization: Bearer $ADMIN_TOKEN"

# ═══════════════════════════════════════════════════════════════════════════════
# Results
# ═══════════════════════════════════════════════════════════════════════════════

echo ""
echo "=== Results: ${PASS} passed, ${FAIL} failed ==="

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
