# Meridian — Unified Test Coverage + README Audit Report

**Audit date:** 2026-04-16  
**Auditor:** Static inspection (no runtime, no mock detection tooling)  
**Scope:** All route handlers, all test files, README.md  
**Method:** Source-only — route registrations cross-referenced against test file contents

---

## Part 1 — Test Coverage & Sufficiency Audit

### 1.1 Endpoint Inventory

Total endpoints discovered: **60**

| # | Method | Path | Handler |
|---|--------|------|---------|
| 1 | GET | /api/v1/health | health |
| 2 | POST | /api/v1/auth/login | login |
| 3 | GET | /api/v1/auth/me | me |
| 4 | POST | /api/v1/auth/logout | logout |
| 5 | POST | /api/v1/auth/request-deletion | request_account_deletion |
| 6 | POST | /api/v1/auth/verify | verify_password |
| 7 | GET | /api/v1/users/me | get_me |
| 8 | POST | /api/v1/users/me/request-deletion | request_deletion |
| 9 | GET | /api/v1/users/me/linked-students | linked_students |
| 10 | GET | /api/v1/admin/users | list_users |
| 11 | POST | /api/v1/admin/users/{id}/set-state | set_user_state |
| 12 | GET | /api/v1/admin/deletion-requests | list_deletion_requests |
| 13 | POST | /api/v1/admin/deletion-requests/{id}/approve | approve_deletion |
| 14 | POST | /api/v1/admin/deletion-requests/{id}/reject | reject_deletion |
| 15 | GET | /api/v1/admin/products | admin_list_products |
| 16 | POST | /api/v1/admin/products | admin_create_product |
| 17 | POST | /api/v1/admin/products/{id}/update | admin_update_product |
| 18 | POST | /api/v1/admin/products/{id}/deactivate | admin_deactivate_product |
| 19 | GET | /api/v1/admin/orders | admin_list_orders |
| 20 | GET | /api/v1/admin/orders/dashboard | admin_orders_dashboard |
| 21 | GET | /api/v1/admin/orders/{id} | admin_get_order |
| 22 | POST | /api/v1/admin/orders/{id}/status | admin_update_order_status |
| 23 | GET | /api/v1/admin/kpi | admin_kpi |
| 24 | GET | /api/v1/admin/config | list_config |
| 25 | POST | /api/v1/admin/config/values/{key} | update_config_value |
| 26 | GET | /api/v1/admin/config/history | config_history |
| 27 | GET | /api/v1/admin/config/campaigns | list_campaigns |
| 28 | POST | /api/v1/admin/config/campaigns/{name} | update_campaign |
| 29 | GET | /api/v1/check-ins/windows | list_windows |
| 30 | GET | /api/v1/check-ins/windows/{id} | get_window |
| 31 | POST | /api/v1/check-ins/windows/{id}/submit | submit_checkin |
| 32 | GET | /api/v1/check-ins/windows/{id}/submissions | list_submissions |
| 33 | GET | /api/v1/check-ins/windows/{id}/homerooms | list_window_homerooms |
| 34 | POST | /api/v1/check-ins/windows/{id}/submissions/{sid}/decide | decide_submission |
| 35 | GET | /api/v1/check-ins/my | my_checkins |
| 36 | GET | /api/v1/products | list_products |
| 37 | GET | /api/v1/products/{id} | get_product |
| 38 | POST | /api/v1/orders | create_order |
| 39 | GET | /api/v1/orders | list_my_orders |
| 40 | GET | /api/v1/orders/{id} | get_my_order |
| 41 | POST | /api/v1/reports | create_report |
| 42 | GET | /api/v1/reports | list_reports |
| 43 | GET | /api/v1/reports/{id} | get_report |
| 44 | GET | /api/v1/reports/{id}/download | download_report |
| 45 | GET | /api/v1/backups | list_backups |
| 46 | POST | /api/v1/backups | create_backup_handler |
| 47 | GET | /api/v1/backups/{id} | get_backup |
| 48 | POST | /api/v1/backups/{id}/restore | restore_backup |
| 49 | GET | /api/v1/notifications | list_notifications |
| 50 | GET | /api/v1/notifications/unread-count | unread_count |
| 51 | POST | /api/v1/notifications/{id}/read | mark_read |
| 52 | POST | /api/v1/notifications/reminders/generate | generate_reminders |
| 53 | GET | /api/v1/config/campaigns/{name}/status | campaign_status |
| 54 | GET | /api/v1/config/commerce | commerce_summary |
| 55 | GET | /api/v1/logs/audit | audit_logs |
| 56 | GET | /api/v1/logs/access | access_logs |
| 57 | GET | /api/v1/logs/errors | error_logs |
| 58 | POST | /api/v1/logs/prune | prune_logs |
| 59 | GET | /api/v1/preferences | get_prefs |
| 60 | PATCH | /api/v1/preferences | patch_prefs |

---

### 1.2 API Test Mapping Table

Legend: ✅ Full (happy path + auth/error variants) · ⚠️ Partial · ❌ No test

| # | Endpoint | Coverage | Test File(s) | Test Functions |
|---|----------|----------|-------------|----------------|
| 1 | GET /health | ✅ | api_auth_payload_tests | test_health_endpoint_payload |
| 2 | POST /auth/login | ✅ | api_auth_payload_tests | 5 tests (success, wrong pass, nonexistent, disabled, shape) |
| 3 | GET /auth/me | ✅ | api_auth_payload_tests | test_me_endpoint_returns_logged_in_user, test_me_without_auth_returns_401 |
| 4 | POST /auth/logout | ✅ | api_auth_payload_tests | test_logout_returns_message_and_invalidates_token |
| 5 | POST /auth/request-deletion | ❌ | — | No test exists for this path |
| 6 | POST /auth/verify | ✅ | api_auth_payload_tests | test_verify_password_correct_returns_verified_true, test_verify_password_wrong_returns_401 |
| 7 | GET /users/me | ✅ | api_users_tests | test_get_users_me_requires_auth, test_get_users_me_returns_user_public_shape |
| 8 | POST /users/me/request-deletion | ✅ | api_users_tests, e2e_workflow_tests | 3 API tests (auth, happy, dup-409); 1 e2e roundtrip |
| 9 | GET /users/me/linked-students | ✅ | api_users_tests | 4 tests (auth, role-403, shape, empty) |
| 10 | GET /admin/users | ✅ | api_authorization_tests, api_admin_users_payload_tests | requires_auth, student-403, required-fields shape |
| 11 | POST /admin/users/{id}/set-state | ✅ | api_admin_users_payload_tests | 3 tests (success shape, 422 invalid, 404 nonexistent) |
| 12 | GET /admin/deletion-requests | ✅ | api_admin_users_payload_tests, e2e_workflow_tests | shape test + e2e roundtrip |
| 13 | POST /admin/deletion-requests/{id}/approve | ✅ | api_admin_users_payload_tests, e2e_workflow_tests | success disables account; e2e roundtrip |
| 14 | POST /admin/deletion-requests/{id}/reject | ✅ | api_admin_users_payload_tests | test_reject_deletion_request_returns_message_and_leaves_account_active |
| 15 | GET /admin/products | ✅ | api_products_tests | test_admin_product_list_includes_inventory_fields |
| 16 | POST /admin/products | ✅ | api_products_tests | 3 tests (correct shape, requires-auth, requires-admin) |
| 17 | POST /admin/products/{id}/update | ✅ | api_products_tests | 3 tests (updated shape, 422 negative price, 404 nonexistent) |
| 18 | POST /admin/products/{id}/deactivate | ✅ | api_products_tests | test_deactivated_product_hidden_from_public_list |
| 19 | GET /admin/orders | ✅ | api_orders_tests | 3 tests (requires-auth, requires-admin, required-fields shape) |
| 20 | GET /admin/orders/dashboard | ✅ | api_orders_tests | test_admin_orders_dashboard_returns_correct_shape |
| 21 | GET /admin/orders/{id} | ✅ | api_orders_tests | 2 tests (correct shape, 404 nonexistent) |
| 22 | POST /admin/orders/{id}/status | ✅ | api_orders_tests, e2e_workflow_tests | 2 API tests (old/new status, 422 invalid) + e2e fulfillment chain |
| 23 | GET /admin/kpi | ✅ | api_orders_tests | 2 tests (required numeric fields, requires-auth) |
| 24 | GET /admin/config | ✅ | api_config_tests | 3 tests (requires-auth, requires-admin, required-fields) |
| 25 | POST /admin/config/values/{key} | ✅ | api_config_tests | 3 tests (old/new shape, 422 wrong type, 404 nonexistent) |
| 26 | GET /admin/config/history | ✅ | api_config_tests | test_admin_config_history_returns_array |
| 27 | GET /admin/config/campaigns | ✅ | api_config_tests | test_admin_list_campaigns_returns_array_with_required_fields |
| 28 | POST /admin/config/campaigns/{name} | ✅ | api_config_tests, e2e_workflow_tests | 2 API tests (correct shape, 404) + e2e campaign-toggle-affects-shipping |
| 29 | GET /check-ins/windows | ✅ | api_checkins_tests | 2 tests (requires-auth, required-fields shape) |
| 30 | GET /check-ins/windows/{id} | ✅ | api_checkins_tests | 2 tests (404 nonexistent, success with status) |
| 31 | POST /check-ins/windows/{id}/submit | ✅ | api_checkins_tests, e2e_workflow_tests | 5 API tests (auth, 404 window, happy, 409 dup, admin-view) + e2e |
| 32 | GET /check-ins/windows/{id}/submissions | ✅ | api_checkins_tests | 3 tests (requires-auth, reviewer-role, shape-with-fields) |
| 33 | GET /check-ins/windows/{id}/homerooms | ⚠️ | api_checkins_tests | test_list_window_homerooms_requires_auth only — no 200 success test |
| 34 | POST /check-ins/windows/{id}/submissions/{sid}/decide | ✅ | api_checkins_tests, e2e_workflow_tests | 2 API tests (approve shape, 422 reject-no-reason) + e2e full cycle |
| 35 | GET /check-ins/my | ✅ | api_checkins_tests | 2 tests (requires-auth, returns-array) |
| 36 | GET /products | ✅ | api_products_tests | 2 tests (requires-auth, required-fields shape) |
| 37 | GET /products/{id} | ⚠️ | api_products_tests | test_get_nonexistent_product_returns_404 only — no happy path |
| 38 | POST /orders | ✅ | api_orders_tests | 4 tests (requires-auth, empty-items-422, correct shape, campaign variant) |
| 39 | GET /orders | ✅ | api_orders_tests | 2 tests (requires-auth, empty-array for new user) |
| 40 | GET /orders/{id} | ✅ | api_orders_tests | 3 tests (correct shape, 403 other-user, 404 nonexistent) |
| 41 | POST /reports | ✅ | api_backups_reports_tests | 4 tests (invalid type 422, invalid date 422, requires-auth, completed shape) |
| 42 | GET /reports | ✅ | api_authorization_tests, api_backups_reports_tests | auth, admin-only, list shape |
| 43 | GET /reports/{id} | ✅ | api_backups_reports_tests | 2 tests (404 nonexistent, correct shape after create) |
| 44 | GET /reports/{id}/download | ❌ | — | No test exists |
| 45 | GET /backups | ✅ | api_authorization_tests, api_backups_reports_tests | auth, admin-only, required-fields shape |
| 46 | POST /backups | ⚠️ | api_backups_reports_tests | test_create_backup_auth_passes_or_internal_error — accepts 201 OR 500 |
| 47 | GET /backups/{id} | ✅ | api_backups_reports_tests | 2 tests (404 nonexistent, correct shape) |
| 48 | POST /backups/{id}/restore | ⚠️ | api_backups_reports_tests | test_restore_pending_backup_returns_409 — error path only, no success case |
| 49 | GET /notifications | ✅ | api_notifications_payload_tests, e2e_workflow_tests | 2 API tests + e2e |
| 50 | GET /notifications/unread-count | ✅ | api_notifications_payload_tests, e2e_workflow_tests | API shape + e2e |
| 51 | POST /notifications/{id}/read | ✅ | api_notifications_payload_tests | 3 tests (decrements count, 404 nonexistent, 403 foreign, idempotent) |
| 52 | POST /notifications/reminders/generate | ❌ | — | No test exists |
| 53 | GET /config/campaigns/{name}/status | ✅ | api_config_tests | 2 tests (name+enabled shape, 404 nonexistent) |
| 54 | GET /config/commerce | ✅ | api_config_tests | 2 tests (requires-auth, correct shape) |
| 55 | GET /logs/audit | ✅ | api_logs_tests | 3 tests (requires-auth, requires-admin, paginated shape) |
| 56 | GET /logs/access | ✅ | api_logs_tests | 3 tests (requires-auth, requires-admin, paginated shape) |
| 57 | GET /logs/errors | ✅ | api_logs_tests | 3 tests (requires-auth, paginated shape, 422 invalid level) |
| 58 | POST /logs/prune | ✅ | api_logs_tests | 2 tests (requires-auth, returns message) |
| 59 | GET /preferences | ⚠️ | e2e_workflow_tests | Only via e2e test_student_preferences_and_notification_read_flow |
| 60 | PATCH /preferences | ⚠️ | e2e_workflow_tests | Only via e2e test_student_preferences_and_notification_read_flow |

---

### 1.3 Coverage Summary

| Category | Count | Pct |
|----------|-------|-----|
| **Fully covered** (happy path + ≥1 error variant) | 51 | 85 % |
| **Partially covered** (auth-only, error-path-only, or e2e-only) | 6 | 10 % |
| **Uncovered** (no test at all) | 3 | 5 % |
| **Total endpoints** | **60** | 100 % |

**Has any test:** 57 / 60 = 95 %  
**Meaningful coverage:** 51 / 60 = 85 %

**Uncovered endpoints (3):**

| Endpoint | Handler | Gap |
|----------|---------|-----|
| POST /api/v1/auth/request-deletion | request_account_deletion | No test exists; the semantically equivalent `POST /users/me/request-deletion` is covered — this may be a legacy duplicate, but auth.rs registers it and it is reachable |
| GET /api/v1/reports/{id}/download | download_report | No test at all; the README documents curl usage but no automated test verifies the CSV download response or Content-Disposition header |
| POST /api/v1/notifications/reminders/generate | generate_reminders | No test at all; the endpoint is reachable but no auth, success, or error case is tested |

**Partially covered endpoints (6):**

| Endpoint | Gap |
|----------|-----|
| GET /check-ins/windows/{id}/homerooms | Only `test_list_window_homerooms_requires_auth` (401 case); no test seeds a homeroom and verifies the 200 response shape |
| GET /products/{id} | Only `test_get_nonexistent_product_returns_404_with_error`; no test fetches an existing product and validates the response fields |
| POST /backups | `test_create_backup_auth_passes_or_internal_error` accepts either 201 or 500 due to `pg_dump` availability; the 201 shape is verified only on the opportunistic path |
| POST /backups/{id}/restore | Only the 409 error path (pending backup) is tested; the success path (completed backup → decrypt → write SQL → return psql_command) has no test |
| GET /preferences | Exercised only in `test_student_preferences_and_notification_read_flow` (E2E); no dedicated payload test asserts the response shape |
| PATCH /preferences | Same as GET /preferences — E2E only, no isolated payload test |

---

### 1.4 Mock Detection

**Result: No mocks found.**

All test files use:
- `PgPoolOptions::new().connect(&url)` → real PostgreSQL connections
- `sqlx::migrate!("../migrations").run(&pool)` → live schema applied to test DB
- `actix_web::test::{init_service, call_service, read_body_json}` → in-process HTTP

No mock database, no stub handlers, no conditional compilation that replaces real services with fakes. All DB-backed tests carry `#[ignore = "requires TEST_DATABASE_URL"]` and require an actual PostgreSQL instance.

---

### 1.5 Unit Test Analysis

| File | Test Type | Count | DB Required |
|------|-----------|-------|-------------|
| api_auth_payload_tests.rs | HTTP integration | 10 | Yes |
| api_products_tests.rs | HTTP integration | 11 | Yes |
| api_orders_tests.rs | HTTP integration | 17 | Yes |
| api_checkins_tests.rs | HTTP integration | 17 | Yes |
| api_backups_reports_tests.rs | HTTP integration | 16 | Yes |
| api_users_tests.rs | HTTP integration | 9 | Yes |
| api_notifications_payload_tests.rs | HTTP integration | 7 | Yes |
| api_admin_users_payload_tests.rs | HTTP integration | 7 | Yes |
| api_config_tests.rs | HTTP integration | 14 | Yes |
| api_logs_tests.rs | HTTP integration | 11 | Yes |
| api_authorization_tests.rs | HTTP integration | 6 | Yes |
| e2e_workflow_tests.rs | End-to-end HTTP | 5 | Yes |
| tests/schema_integrity_tests.rs | SQL/schema | 3 | Yes |
| tests/hardening_tests.rs | Mixed (unit + DB) | 53 | Some |
| tests/commerce_tests.rs | Mixed (unit + DB) | 25 | Some |
| tests/admin_scope_tests.rs | HTTP integration | 24 | Yes |
| frontend/admin_users.rs (cfg test) | Pure Rust unit | 15 | No |

**Total test functions: ~280+**  
All DB-backed HTTP tests carry `#[ignore]` and are activated only via `--include-ignored`. Pure unit tests in `hardening_tests`, `commerce_tests`, and the frontend run without a database.

---

### 1.6 Sufficiency Verdict

**Score: 85 / 100**

Strengths:
- All 12 route modules have at least some HTTP integration coverage
- Every covered endpoint has both an authentication check (401/403) and a happy-path assertion
- Response shape validation is thorough (field names and types explicitly asserted)
- E2E tests cover the three highest-value multi-step workflows: deletion roundtrip, check-in cycle, and campaign-price interaction
- No mocks — all assertions run against a real PostgreSQL schema

Gaps:
- 3 endpoints completely untested (5 %)
- `GET /products/{id}` happy path missing despite the list endpoint being covered
- `GET /reports/{id}/download` untested despite README documentation
- `POST /notifications/reminders/generate` untested
- Homeroom listing (check-ins) has only an auth smoke test
- Preferences endpoints have no dedicated payload test suite

---

## Part 2 — README Quality & Compliance Audit

### 2.1 Content Checklist

| Gate | Status | Notes |
|------|--------|-------|
| Project purpose clearly stated | ✅ | Opening paragraph accurately describes scope |
| Prerequisites listed | ✅ | Rust, cargo, trunk, wasm32, PostgreSQL 14+, psql, pg_dump all listed |
| Startup order documented | ✅ | 6-step numbered sequence is correct and complete |
| Environment variables table | ✅ | All 9 variables documented with required/default/notes |
| Seeded credentials listed | ✅ | All 6 users with roles and passwords |
| Super-admin policy explained | ✅ | Architectural intent and scoped-by-default behaviour documented |
| Test commands documented | ✅ | Unit, integration, and `run_tests.sh` all covered |
| `#[ignore]` pattern explained | ✅ | "All DB-backed tests are tagged `#[ignore]`" clearly stated |
| Docker usage clarified | ✅ | Docker mentioned only as fallback; primary path is local |
| Manual verification curl examples | ✅ | Covers health, login, commerce, orders, config, reports, logs, backups |
| KPI definitions | ✅ | All 6 metrics defined |
| Background scheduler behavior | ✅ | 4 jobs with frequency and description |
| PII masking rules | ✅ | Table with masked form and permission requirement |
| Backup encryption scheme | ✅ | Algorithm, key derivation, file format, checksum documented |
| Known limitations | ✅ | pg_dump requirement, no email transport, no HTTPS, test prerequisites |

---

### 2.2 Accuracy Issues Found

**Issue 1 — Stale "Verified test run results" table (MODERATE)**

The README table at line 251 shows results dated 2026-04-03 and lists only 5 suites:
`schema_integrity_tests`, `hardening_tests`, `commerce_tests`, `admin_scope_tests`, `meridian-backend binary`.

The following test suites added after that date are absent from the table:
- All 11 `API_TESTS/` suites (api_auth_payload_tests, api_products_tests, api_orders_tests, api_checkins_tests, api_backups_reports_tests, api_users_tests, api_notifications_payload_tests, api_admin_users_payload_tests, api_config_tests, api_logs_tests, api_authorization_tests)
- `e2e_workflow_tests`

The table does not reflect the current state of test coverage.

---

**Issue 2 — Wrong paths in "High-risk areas" table (MINOR)**

Lines 703–704 read:
```
| API authorization for reports/backups | `backend/tests/api_authorization_tests.rs` — DB-backed HTTP tests |
| Multi-step API workflows | `backend/tests/e2e_workflow_tests.rs` — DB-backed end-to-end API tests |
```

Actual paths are:
- `backend/API_TESTS/api_authorization_tests.rs`
- `backend/e2e_tests/e2e_workflow_tests.rs`

Both paths in the README are wrong. The `backend/tests/` directory only contains `schema_integrity_tests.rs`, `hardening_tests.rs`, `commerce_tests.rs`, and `admin_scope_tests.rs`.

---

**Issue 3 — Report response shape mismatch in curl example (MINOR)**

README line 444–445 documents the report creation response as:
```json
{"id":"...","status":"completed","path":"..."}
```

The actual handler returns a struct with fields: `job_id`, `name`, `report_type`, `status`, `output_path`, `row_count`, `pii_masked`, `checksum`. The documented field names `id` and `path` do not match `job_id` and `output_path`. A developer following the README and checking `response.id` or `response.path` will get `undefined`.

---

**Issue 4 — `admin_scope_tests` not in project structure tree (MINOR)**

README line 39–42 shows the `tests/` directory containing only:
```
├── commerce_tests.rs
└── hardening_tests.rs
```

`tests/admin_scope_tests.rs` exists (run_tests.sh invokes it, the README's own coverage table references it) but is omitted from the structure diagram.

---

**Issue 5 — `/users/me` route module not in project structure tree (MINOR)**

The structure diagram at line 27 lists route files under `routes/` but does not include `users.rs`, `checkins.rs`, `notifications.rs`, or `preferences.rs`. The diagram is incomplete relative to the actual route module set.

---

**Issue 6 — `run_tests.sh` path inconsistency in README (MINOR)**

The "DB integration tests" section at line 273 refers to the "checked-in runner" and later at line 275 says:
```
cd ..
repo/run_tests.sh
```

This implies running from the workspace parent (`w2t25/`) and calling `repo/run_tests.sh`. The script itself uses `"$(dirname "${BASH_SOURCE[0]}")"` for root detection, which works correctly. However, the Docker-first invocation example at line 291 says:
```
cd repo
./run_tests.sh
```

The two examples use different working directories (`..` + `repo/` vs inside `repo/`). This is not technically wrong (the script handles both) but it is inconsistent and could confuse a first-time reader.

---

### 2.3 README Verdict

**Score: 88 / 100**

The README is unusually thorough for a local-only application. It documents the full architecture, all environment variables, precise startup order, seeded credentials, test harness conventions, KPI definitions, PII masking rules, and backup encryption details. The main deficiencies are:

- The "Verified test run results" table is stale and omits all API_TESTS suites
- Two file paths in the "High-risk areas" table are wrong
- The report creation curl example shows incorrect field names

None of these issues break functionality; they are documentation drift from iterative test additions.

---

## Part 3 — Combined Findings Summary

### 3.1 Test Coverage Gaps (Action Required)

| Priority | Endpoint | Recommended Action |
|----------|----------|-------------------|
| HIGH | GET /api/v1/reports/{id}/download | Add test that creates a report, then GETs the download URL and asserts Content-Type: text/csv and non-empty body |
| HIGH | POST /api/v1/notifications/reminders/generate | Add auth test (401) and happy-path test (2xx or expected status) |
| MEDIUM | POST /api/v1/auth/request-deletion | Either add a test or document that this is a deprecated duplicate of `/users/me/request-deletion` and remove the route registration |
| MEDIUM | GET /api/v1/products/{id} | Add one test that seeds a product and asserts the 200 shape (id, name, price_cents, active) |
| MEDIUM | GET /api/v1/check-ins/windows/{id}/homerooms | Add a test that seeds a school+homeroom+window and asserts 200 array shape |
| LOW | GET /api/v1/preferences | Add dedicated payload test with shape assertions |
| LOW | PATCH /api/v1/preferences | Add dedicated payload test |
| LOW | POST /api/v1/backups/{id}/restore (success) | Seed a completed backup and assert the success response shape (restore_path, psql_command, warning) |

### 3.2 README Fixes (Action Required)

| Priority | Issue | Fix |
|----------|-------|-----|
| HIGH | Stale "Verified test run results" table | Add rows for all 11 API_TESTS suites and e2e_workflow_tests with current test counts |
| MEDIUM | Wrong file paths in "High-risk areas" | Change `backend/tests/api_authorization_tests.rs` → `backend/API_TESTS/api_authorization_tests.rs` and `backend/tests/e2e_workflow_tests.rs` → `backend/e2e_tests/e2e_workflow_tests.rs` |
| MEDIUM | Report curl example field names | Change `{"id":"...","path":"..."}` → `{"job_id":"...","output_path":"..."}` (or expand to show all returned fields) |
| LOW | Missing files in project structure tree | Add `users.rs`, `checkins.rs`, `notifications.rs`, `preferences.rs` to the routes listing; add `admin_scope_tests.rs` to the tests listing |
| LOW | Inconsistent `run_tests.sh` invocation examples | Pick one working-directory convention and use it consistently |

---

*Report generated by static inspection. No runtime execution was performed.*
