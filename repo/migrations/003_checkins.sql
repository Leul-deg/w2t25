-- Check-in windows (defines when check-ins are open)
CREATE TABLE IF NOT EXISTS checkin_windows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    school_id UUID NOT NULL REFERENCES schools(id) ON DELETE CASCADE,
    class_id UUID REFERENCES classes(id),
    title TEXT NOT NULL,
    description TEXT,
    opens_at TIMESTAMPTZ NOT NULL,
    closes_at TIMESTAMPTZ NOT NULL,
    allow_late BOOLEAN NOT NULL DEFAULT FALSE,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    CHECK (closes_at > opens_at)
);

-- Check-in submissions
CREATE TABLE IF NOT EXISTS checkin_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    window_id UUID NOT NULL REFERENCES checkin_windows(id) ON DELETE CASCADE,
    student_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    method TEXT NOT NULL DEFAULT 'manual' CHECK (method IN ('manual', 'qr', 'badge', 'parent')),
    notes TEXT,
    ip_address TEXT,
    is_late BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (window_id, student_id)
);

-- Approval decisions on check-ins
CREATE TABLE IF NOT EXISTS checkin_approval_decisions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    submission_id UUID NOT NULL REFERENCES checkin_submissions(id) ON DELETE CASCADE UNIQUE,
    decided_by UUID NOT NULL REFERENCES users(id),
    decision TEXT NOT NULL CHECK (decision IN ('approved', 'rejected', 'pending')),
    reason TEXT,
    decided_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_checkin_windows_school_id ON checkin_windows(school_id);
CREATE INDEX IF NOT EXISTS idx_checkin_windows_opens_at ON checkin_windows(opens_at);
CREATE INDEX IF NOT EXISTS idx_checkin_windows_closes_at ON checkin_windows(closes_at);
CREATE INDEX IF NOT EXISTS idx_checkin_windows_active ON checkin_windows(active);
CREATE INDEX IF NOT EXISTS idx_checkin_submissions_window_id ON checkin_submissions(window_id);
CREATE INDEX IF NOT EXISTS idx_checkin_submissions_student_id ON checkin_submissions(student_id);
CREATE INDEX IF NOT EXISTS idx_checkin_submissions_submitted_at ON checkin_submissions(submitted_at);
