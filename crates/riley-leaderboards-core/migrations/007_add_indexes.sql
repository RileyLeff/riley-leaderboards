-- Performance indexes for pagination, "latest version", and hot query paths.

-- Versions: "latest version for board" and version listing (DESC by number)
CREATE INDEX idx_versions_board_id_version_number
    ON versions (board_id, version_number DESC);

-- Versions: cursor pagination uses (created_at, id) ordering
CREATE INDEX idx_versions_board_id_created_at_id
    ON versions (board_id, created_at DESC, id DESC);

-- Entries: cursor pagination uses (created_at, id) ordering
CREATE INDEX idx_entries_board_id_created_at_id
    ON entries (board_id, created_at ASC, id ASC);

-- Placements: always fetched by version_id, ordered by position
CREATE INDEX idx_placements_version_id_position
    ON placements (version_id, COALESCE(position, 2147483647) ASC);

-- Accumulated scores: snapshot queries filter by board_id
CREATE INDEX idx_accumulated_scores_board_id
    ON accumulated_scores (board_id);

-- Board references: cursor pagination uses (created_at, id) ordering
CREATE INDEX idx_board_references_board_id_created_at_id
    ON board_references (board_id, created_at ASC, id ASC);
