# Fixed Issues And How They Were Fixed

Based on the progression from `delivery-acceptance-architecture-audit.md` through `delivery-acceptance-architecture-audit-06.md`.

- **Scoped-admin RBAC gaps**
  - Original issue: the first audit reported scoped-admin bypasses in check-in access, order detail, product reads, and config/history/campaign reads.
  - How it was fixed: later audits confirmed scope checks were added to the affected backend paths and backed by new backend regression tests.

- **LAN/local-network API support**
  - Original issue: the first audit reported localhost-only frontend/API coupling.
  - How it was fixed: later audits confirmed the frontend derived the API host from the browser hostname and the backend CORS policy was updated for LAN usage.

- **Admin user/deletion frontend pages**
  - Original issue: the first audit reported `AdminUsers` and `AdminDeletionRequests` as placeholders.
  - How it was fixed: later audits confirmed both pages were implemented and wired into the admin shell.

- **Session configuration wiring**
  - Original issue: the first audit reported `SESSION_SECRET` and `SESSION_MAX_AGE_SECONDS` as documented but unused.
  - How it was fixed: later audits confirmed the auth flow used configurable TTL and secret-influenced token generation.

- **README/manual verification mismatches**
  - Original issue: the first audit reported incorrect methods/paths in the README.
  - How it was fixed: later audits confirmed the admin/config verification examples were aligned with the actual routes.

- **Large parts of the test suite mirrored intended behavior**
  - Original issue: the first audit reported that many top-level tests mirrored expected behavior instead of exercising production code.
  - How it was fixed: later audits confirmed backend tests gained direct production-code reuse through `backend/src/lib.rs`, reducing some duplication and improving the rigor of parts of the suite.

- **Admin dashboard card label/route mismatch**
  - Original issue: the first audit reported inconsistent admin dashboard card labels and destinations.
  - How it was fixed: later audits confirmed the admin homepage cards were realigned to their intended routes and descriptions.
