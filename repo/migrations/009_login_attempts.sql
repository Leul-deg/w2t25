-- Login attempt tracking for rate limiting
CREATE TABLE IF NOT EXISTS login_attempts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username TEXT NOT NULL,
    ip_address TEXT,
    attempted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    success BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_login_attempts_username_time
    ON login_attempts (username, attempted_at DESC);
CREATE INDEX IF NOT EXISTS idx_login_attempts_ip_time
    ON login_attempts (ip_address, attempted_at DESC);
