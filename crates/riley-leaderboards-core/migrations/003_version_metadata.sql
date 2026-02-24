-- Add metadata JSONB column to versions for structured context
-- (blog post URLs, changelogs, publication timestamps, etc.)
ALTER TABLE versions ADD COLUMN metadata jsonb;
