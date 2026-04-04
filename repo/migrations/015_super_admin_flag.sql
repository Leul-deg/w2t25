-- Migration 015: explicit super-admin flag on users.
--
-- Prior behaviour: "no rows in admin_scope_assignments" was treated as
-- super-admin (unrestricted).  This is bypassable — a newly-created admin
-- account with no scope rows immediately had unrestricted access.
--
-- New behaviour:
--   is_super_admin = true  → unrestricted; may perform global operations.
--   is_super_admin = false → must have explicit scope rows; no scope rows
--                            means zero-access (empty campus list returned).
--
-- The seeded admin_user (00000000-0000-0000-0000-000000000001) is marked
-- is_super_admin = true so existing dev environments are unaffected.
-- All other existing admin accounts default to false (scoped-by-default).

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS is_super_admin BOOLEAN NOT NULL DEFAULT false;

-- Mark the canonical seed super-admin.  In production this UPDATE should be
-- run deliberately for each account that truly requires unrestricted access.
UPDATE users SET is_super_admin = true
WHERE id = '00000000-0000-0000-0000-000000000001';

CREATE INDEX IF NOT EXISTS idx_users_is_super_admin ON users(is_super_admin)
    WHERE is_super_admin = true;
