-- User notification preferences (one row per user, created on first save)
CREATE TABLE IF NOT EXISTS user_preferences (
    user_id       UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    notif_checkin BOOLEAN NOT NULL DEFAULT TRUE,
    notif_order   BOOLEAN NOT NULL DEFAULT TRUE,
    notif_general BOOLEAN NOT NULL DEFAULT TRUE,
    dnd_enabled   BOOLEAN NOT NULL DEFAULT FALSE,
    -- DND window is stored as UTC clock times.
    -- Default: 21:00–06:00 (overnight).
    dnd_start     TIME    NOT NULL DEFAULT '21:00:00',
    dnd_end       TIME    NOT NULL DEFAULT '06:00:00',
    -- "immediate" | "daily" | "weekly"
    inbox_frequency TEXT  NOT NULL DEFAULT 'immediate'
        CHECK (inbox_frequency IN ('immediate', 'daily', 'weekly')),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Allow notifications to be deferred until a future time.
-- NULL means "display immediately".
ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS display_after TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_notifications_display_after
    ON notifications(display_after);

-- Reference key for deduplication of auto-generated reminders.
ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS ref_key TEXT;

CREATE INDEX IF NOT EXISTS idx_notifications_ref_key
    ON notifications(recipient_id, ref_key)
    WHERE ref_key IS NOT NULL;
