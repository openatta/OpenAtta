-- Remote Agent 注册表
CREATE TABLE IF NOT EXISTS remote_agents (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    token_hash      TEXT NOT NULL UNIQUE,
    description     TEXT NOT NULL DEFAULT '',
    version         TEXT NOT NULL DEFAULT '0.1.0',
    capabilities    TEXT NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'offline',
    last_heartbeat  TEXT,
    registered_at   TEXT NOT NULL,
    registered_by   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ra_token ON remote_agents(token_hash);
CREATE INDEX IF NOT EXISTS idx_ra_status ON remote_agents(status);
