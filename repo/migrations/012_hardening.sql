-- ── 012 Hardening: permissions, indexes, retention config, schema fixes ──────

-- 1. PII Export permission --------------------------------------------------
INSERT INTO permissions (name, description, resource, action)
VALUES ('pii_export', 'Allows exporting reports with unmasked PII', 'reports', 'pii_export')
ON CONFLICT (name) DO NOTHING;

-- Grant PII Export permission to the Administrator role
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM   roles r, permissions p
WHERE  r.name = 'Administrator' AND p.name = 'pii_export'
ON CONFLICT DO NOTHING;

-- 2. Make report_jobs.requested_by nullable so the scheduler can create jobs --
ALTER TABLE report_jobs ALTER COLUMN requested_by DROP NOT NULL;

-- 3. Add retention_days config value (180-day default) ----------------------
INSERT INTO config_values (id, key, value, value_type, description, scope)
VALUES (gen_random_uuid(), 'log_retention_days', '180', 'integer',
        'Number of days to retain access/audit/error log entries', 'global')
ON CONFLICT (key) DO NOTHING;

-- 4. Add backup_encryption_key_hint config (non-secret label only) ----------
INSERT INTO config_values (id, key, value, value_type, description, scope)
VALUES (gen_random_uuid(), 'backup_encryption_hint', 'BACKUP_ENCRYPTION_KEY env var', 'string',
        'Documents which env variable holds the AES-256 backup key', 'global')
ON CONFLICT (key) DO NOTHING;

-- 5. Indexes for report date-range queries -----------------------------------
-- check-in submissions
CREATE INDEX IF NOT EXISTS idx_checkin_submissions_submitted_at
    ON checkin_submissions(submitted_at);
-- orders date range
CREATE INDEX IF NOT EXISTS idx_orders_confirmed_fulfilled
    ON orders(created_at) WHERE status IN ('confirmed', 'fulfilled');
-- audit logs by action for faster report queries
CREATE INDEX IF NOT EXISTS idx_audit_logs_action
    ON audit_logs(action);
-- access logs failures only
CREATE INDEX IF NOT EXISTS idx_access_logs_failures
    ON access_logs(created_at) WHERE success = FALSE;

-- 6. Fix blacklist_entries column name bug in existing code -----------------
-- The admin.rs INSERT references "created_at" but the column is "blacklisted_at".
-- Add an alias column so both names work without breaking anything.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE  table_name = 'blacklist_entries' AND column_name = 'created_at'
    ) THEN
        ALTER TABLE blacklist_entries ADD COLUMN created_at TIMESTAMPTZ DEFAULT NOW();
    END IF;
END
$$;

-- 7. Add pii_masked flag to report_jobs so we know how each was generated ---
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE  table_name = 'report_jobs' AND column_name = 'pii_masked'
    ) THEN
        ALTER TABLE report_jobs ADD COLUMN pii_masked BOOLEAN NOT NULL DEFAULT TRUE;
    END IF;
END
$$;

-- 8. Add checksum to report_jobs for integrity verification -----------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE  table_name = 'report_jobs' AND column_name = 'checksum'
    ) THEN
        ALTER TABLE report_jobs ADD COLUMN checksum TEXT;
    END IF;
END
$$;
