-- Add source field to skill_defs and flow_defs tables
-- "builtin" for built-in definitions, "imported" for user-imported ones

ALTER TABLE skill_defs ADD COLUMN source TEXT NOT NULL DEFAULT 'builtin';
ALTER TABLE flow_defs ADD COLUMN source TEXT NOT NULL DEFAULT 'builtin';
