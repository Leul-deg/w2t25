-- Report jobs
CREATE TABLE IF NOT EXISTS report_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    report_type TEXT NOT NULL,
    parameters JSONB,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'completed', 'failed', 'cancelled')),
    requested_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    output_path TEXT,
    error_message TEXT,
    row_count INTEGER
);

-- Backup metadata
CREATE TABLE IF NOT EXISTS backup_metadata (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    filename TEXT NOT NULL UNIQUE,
    backup_type TEXT NOT NULL DEFAULT 'full' CHECK (backup_type IN ('full', 'incremental', 'schema')),
    size_bytes BIGINT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'completed', 'failed', 'deleted')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    checksum TEXT,
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_report_jobs_status ON report_jobs(status);
CREATE INDEX IF NOT EXISTS idx_report_jobs_requested_by ON report_jobs(requested_by);
CREATE INDEX IF NOT EXISTS idx_report_jobs_created_at ON report_jobs(created_at);
CREATE INDEX IF NOT EXISTS idx_backup_metadata_status ON backup_metadata(status);
CREATE INDEX IF NOT EXISTS idx_backup_metadata_created_at ON backup_metadata(created_at);
