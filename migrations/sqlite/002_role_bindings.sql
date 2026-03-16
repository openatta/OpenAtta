-- Role bindings table for RBAC
-- Maps actor IDs to roles for authorization checks

CREATE TABLE IF NOT EXISTS role_bindings (
    actor_id TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (actor_id, role)
);

CREATE INDEX IF NOT EXISTS idx_role_bindings_actor ON role_bindings(actor_id);
