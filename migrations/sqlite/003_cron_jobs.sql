-- Cron jobs and execution history tables

CREATE TABLE IF NOT EXISTS cron_jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    schedule TEXT NOT NULL,           -- cron expression (e.g., "0 */5 * * * *")
    command TEXT NOT NULL,            -- command/action to execute
    config TEXT NOT NULL DEFAULT '{}', -- JSON config (flow_id, skill, args, etc.)
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by TEXT NOT NULL DEFAULT 'system',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_run_at TEXT,
    next_run_at TEXT
);

CREATE TABLE IF NOT EXISTS cron_runs (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES cron_jobs(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'running',  -- running, completed, failed
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    output TEXT,
    error TEXT,
    triggered_by TEXT NOT NULL DEFAULT 'scheduler' -- scheduler, manual
);

CREATE INDEX IF NOT EXISTS idx_cron_runs_job ON cron_runs(job_id);
CREATE INDEX IF NOT EXISTS idx_cron_runs_started ON cron_runs(started_at);
