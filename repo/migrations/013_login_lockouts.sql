-- Explicit per-username lockout state for brute-force protection.
-- Complements login_attempts (rolling window count); this table holds the
-- persisted "locked until" timestamp so the 30-minute lockout survives
-- restarts and is enforced independently of the rolling count.
CREATE TABLE IF NOT EXISTS login_lockouts (
    username    TEXT PRIMARY KEY,
    locked_until TIMESTAMPTZ NOT NULL
);
