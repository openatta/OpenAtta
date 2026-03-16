-- Add version column to tasks for optimistic concurrency control
ALTER TABLE tasks ADD COLUMN version INTEGER NOT NULL DEFAULT 0;
