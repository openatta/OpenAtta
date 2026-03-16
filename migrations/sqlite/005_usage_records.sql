-- Usage tracking for LLM API calls

CREATE TABLE IF NOT EXISTS usage_records (
    id TEXT PRIMARY KEY,
    task_id TEXT,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_usage_created ON usage_records(created_at);
CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model);
CREATE INDEX IF NOT EXISTS idx_usage_task ON usage_records(task_id);
