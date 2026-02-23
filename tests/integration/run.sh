#!/usr/bin/env bash
# Integration test: builds the Docker image, starts the service + Postgres,
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

# --- Health check ---
RESP=$(curl -sf "$BASE_URL/health")
STATUS=$(echo "$RESP" | jq -r '.status')
if [ "$STATUS" = "ok" ]; then
    pass "GET /health returns ok"
else
    fail "GET /health" "expected status=ok, got $STATUS"
fi

# --- Create a board ---
RESP=$(curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -d '{"slug":"int-test-board","name":"Integration Test Board","board_type":"scored","sort_direction":"desc"}')
SLUG=$(echo "$RESP" | jq -r '.slug')
if [ "$SLUG" = "int-test-board" ]; then
    pass "POST /boards creates board"
else
    fail "POST /boards" "expected slug=int-test-board, got $SLUG"
fi

# --- List boards ---
RESP=$(curl -sf "$BASE_URL/boards")
COUNT=$(echo "$RESP" | jq '.items | length')
if [ "$COUNT" -ge 1 ]; then
    pass "GET /boards lists boards"
else
    fail "GET /boards" "expected >=1 board, got $COUNT"
fi

# --- Get board ---
RESP=$(curl -sf "$BASE_URL/boards/int-test-board")
NAME=$(echo "$RESP" | jq -r '.name')
if [ "$NAME" = "Integration Test Board" ]; then
    pass "GET /boards/:slug returns board"
else
    fail "GET /boards/:slug" "expected name='Integration Test Board', got $NAME"
fi

# --- Create entries ---
curl -sf -X POST "$BASE_URL/boards/int-test-board/entries" \
    -H "Content-Type: application/json" \
    -d '{"slug":"entry-a","name":"Entry A"}' >/dev/null
curl -sf -X POST "$BASE_URL/boards/int-test-board/entries" \
    -H "Content-Type: application/json" \
    -d '{"slug":"entry-b","name":"Entry B"}' >/dev/null
pass "POST /boards/:slug/entries creates entries"

# --- Create version with placements ---
RESP=$(curl -sf -X POST "$BASE_URL/boards/int-test-board/versions" \
    -H "Content-Type: application/json" \
    -d '{"placements":[{"entry_slug":"entry-a","score":100.0},{"entry_slug":"entry-b","score":200.0}]}')
VER=$(echo "$RESP" | jq '.version_number')
if [ "$VER" = "1" ]; then
    pass "POST /boards/:slug/versions creates version"
else
    fail "POST /boards/:slug/versions" "expected version_number=1, got $VER"
fi

# --- Get latest version ---
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/latest")
PLACEMENT_COUNT=$(echo "$RESP" | jq '.placements | length')
if [ "$PLACEMENT_COUNT" = "2" ]; then
    pass "GET /boards/:slug/latest returns placements"
else
    fail "GET /boards/:slug/latest" "expected 2 placements, got $PLACEMENT_COUNT"
fi

# --- Verify scored positions (desc: higher score = position 1) ---
POS1=$(echo "$RESP" | jq '[.placements[] | select(.entry_slug=="entry-b")] | .[0].position')
if [ "$POS1" = "1" ]; then
    pass "Scored board derives positions correctly (desc)"
else
    fail "Scored positions" "expected entry-b at position 1, got $POS1"
fi

# --- Entry history ---
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/entries/entry-a/history")
HIST_COUNT=$(echo "$RESP" | jq 'length')
if [ "$HIST_COUNT" -ge 1 ]; then
    pass "GET /boards/:slug/entries/:entry/history works"
else
    fail "Entry history" "expected >=1 history entry, got $HIST_COUNT"
fi

# --- Version diff (create second version) ---
curl -sf -X POST "$BASE_URL/boards/int-test-board/versions" \
    -H "Content-Type: application/json" \
    -d '{"placements":[{"entry_slug":"entry-a","score":300.0},{"entry_slug":"entry-b","score":200.0}]}' >/dev/null
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/diff?from=1&to=2")
MOVED=$(echo "$RESP" | jq '.moved | length')
if [ "$MOVED" -ge 1 ]; then
    pass "GET /boards/:slug/diff returns changes"
else
    fail "Version diff" "expected >=1 moved entry, got $MOVED"
fi

# --- Since endpoint ---
RESP=$(curl -sf "$BASE_URL/boards/int-test-board/since/1")
SINCE_COUNT=$(echo "$RESP" | jq 'length')
if [ "$SINCE_COUNT" -ge 1 ]; then
    pass "GET /boards/:slug/since/:v returns newer versions"
else
    fail "Since endpoint" "expected >=1 version, got $SINCE_COUNT"
fi

# --- Pagination (create a second board so there's a next page) ---
curl -sf -X POST "$BASE_URL/boards" \
    -H "Content-Type: application/json" \
    -d '{"slug":"pagination-board","name":"Pagination Board","board_type":"ordered","sort_direction":"asc"}' >/dev/null
RESP=$(curl -sf "$BASE_URL/boards?limit=1")
ITEMS=$(echo "$RESP" | jq '.items | length')
CURSOR=$(echo "$RESP" | jq -r '.next_cursor')
if [ "$ITEMS" = "1" ] && [ "$CURSOR" != "null" ]; then
    pass "Pagination works (limit=1, cursor returned)"
else
    fail "Pagination" "expected 1 item with cursor, got items=$ITEMS cursor=$CURSOR"
fi
# Clean up pagination board
curl -sf -o /dev/null -X DELETE "$BASE_URL/boards/pagination-board"

# --- Delete board ---
HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" -X DELETE "$BASE_URL/boards/int-test-board")
if [ "$HTTP_CODE" = "204" ]; then
    pass "DELETE /boards/:slug succeeds"
else
    fail "DELETE /boards/:slug" "expected 204, got $HTTP_CODE"
fi

# --- Verify deletion ---
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/boards/int-test-board")
if [ "$HTTP_CODE" = "404" ]; then
    pass "Deleted board returns 404"
else
    fail "Deletion verification" "expected 404, got $HTTP_CODE"
fi

echo ""
echo "=== Results: ${PASS} passed, ${FAIL} failed ==="

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
