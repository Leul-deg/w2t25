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

- **Checked-in CI workflow**
  - Original issue: earlier audits noted there was no checked-in CI workflow file.
  - How it was fixed: later audits confirmed `.github/workflows/ci.yml` was added and invoked the project runner.

- **Checked-in runner coverage**
  - Original issue: earlier audits flagged that the default runner only did backend `cargo test` and frontend compile checks.
  - How it was fixed: later audits confirmed `run_tests.sh` was expanded to automate the ignored DB-backed suites, and then also to run frontend `cargo test`.

- **Backend production-code reuse in tests**
  - Original issue: earlier audits flagged heavy mirror-style testing.
  - How it was fixed: later audits confirmed `backend/src/lib.rs` was added so test suites could import production modules directly.

- **Frontend unit coverage**
  - Original issue: earlier audits said frontend had no automated tests.
  - How it was fixed: later audits confirmed inline frontend unit tests were added for route classification, admin helper behavior, dashboard routing, input normalization, and API-base construction.

- **README checked-in runner numbering**
  - Original issue: a later audit still noted a numbering typo in the runner section.
  - How it was fixed: the latest audit no longer reported that issue.
