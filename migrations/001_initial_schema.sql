-- riley_leaderboards v1 schema
-- Requires PostgreSQL 18 (native uuidv7)

-- Boards
CREATE TABLE boards (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    slug text UNIQUE NOT NULL,
    name text NOT NULL,
    board_type text NOT NULL,
    sort_direction text NOT NULL DEFAULT 'desc',
    tier_config jsonb,
    metadata jsonb,
    accumulative boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Entries (belong to a board, persist across versions)
CREATE TABLE entries (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    slug text NOT NULL,
    name text NOT NULL,
    metadata jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (board_id, slug)
);

CREATE INDEX idx_entries_board_id ON entries(board_id);

-- Versions (immutable snapshots of a board's rankings)
CREATE TABLE versions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    version_number integer NOT NULL,
    note text,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (board_id, version_number)
);

CREATE INDEX idx_versions_board_id_number ON versions(board_id, version_number);

-- Placements (join between entry and version)
CREATE TABLE placements (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    version_id uuid NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    entry_id uuid NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    position integer,
    score double precision,
    tier text,
    metadata jsonb,
    UNIQUE (version_id, entry_id)
);

CREATE INDEX idx_placements_version_id ON placements(version_id);
CREATE INDEX idx_placements_entry_id ON placements(entry_id);

-- Board references (links between board versions and external contexts)
-- Named "board_references" because "references" is a SQL reserved keyword.
CREATE TABLE board_references (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    pinned_version_id uuid REFERENCES versions(id) ON DELETE SET NULL,
    uri text NOT NULL,
    ref_type text NOT NULL,
    label text,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_board_references_board_id ON board_references(board_id);

-- Accumulated scores (staging area for accumulative boards)
CREATE TABLE accumulated_scores (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    entry_id uuid NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    score double precision NOT NULL,
    submitted_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (board_id, entry_id)
);

CREATE INDEX idx_accumulated_scores_board_id ON accumulated_scores(board_id);
