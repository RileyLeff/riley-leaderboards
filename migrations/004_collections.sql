-- Board collections: group related boards for index pages and navigation.

CREATE TABLE collections (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    slug text UNIQUE NOT NULL,
    name text NOT NULL,
    metadata jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE collection_boards (
    collection_id uuid NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    board_id uuid NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    display_order integer NOT NULL DEFAULT 0,
    PRIMARY KEY (collection_id, board_id)
);
