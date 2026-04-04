-- Per-admin district/campus scope assignments.
--
-- NOTE: the original "no rows = super-admin" comment below was superseded by
-- migration 015 (is_super_admin flag).  Current semantics:
--   users.is_super_admin = true  → unrestricted access (ignores this table).
--   users.is_super_admin = false → access is limited to the districts/campuses
--                                  listed here; no rows means zero access
--                                  (scoped-by-default).
--
-- An admin with rows here may only manage users/orders within the listed
-- districts (expanded to their campuses) or campuses directly.
CREATE TABLE IF NOT EXISTS admin_scope_assignments (
    admin_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope_type  TEXT NOT NULL CHECK (scope_type IN ('district', 'campus')),
    scope_id    UUID NOT NULL,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (admin_id, scope_type, scope_id)
);

CREATE INDEX IF NOT EXISTS idx_admin_scope_assignments_admin_id
    ON admin_scope_assignments(admin_id);
