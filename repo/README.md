# Meridian — Check-In & Commerce Operations Suite

Offline-first, local-only application for school district check-in operations,
integrated merchandise store, and reporting/admin workflow.

**Stack:** Rust · Actix-web · SQLx · PostgreSQL · Yew (WASM)

---

## Project structure

```
meridian/
├── backend/          Actix-web REST API
│   ├── src/
│   │   ├── main.rs           Server entry point + scheduler spawn
│   │   ├── config.rs         Environment config loader
│   │   ├── db.rs             Connection pool + migration runner
│   │   ├── errors.rs         Typed AppError with HTTP mapping
│   │   ├── middleware/       Auth extraction helpers
│   │   ├── models/           User, Role, Session structs
│   │   ├── routes/           API handlers
│   │   │   ├── auth.rs       Login, logout, me
│   │   │   ├── admin.rs      Admin console (products, orders, users, KPI)
│   │   │   ├── products.rs   Public product listing
│   │   │   ├── orders.rs     Customer order creation and history
│   │   │   ├── config_routes.rs  Config values and campaign toggles
│   │   │   ├── reports.rs    Report generation and download
│   │   │   ├── logs.rs       Audit, access, and error log viewer
│   │   │   └── backups.rs    Encrypted backup and restore preparation
│   │   └── services/         Business logic
│   │       ├── auth.rs       Password hashing (Argon2id)
│   │       ├── backup.rs     AES-256-GCM encrypt/decrypt + pg_dump
│   │       ├── commerce.rs   Shipping fee, points, totals
│   │       ├── masking.rs    PII masking + pii_export permission check
│   │       ├── reports.rs    CSV generation for all report types
│   │       ├── scheduler.rs  Background job runner (auto-close, prune, reports)
│   │       └── notifications.rs  Notification helpers
│   ├── tests/
│   │   ├── commerce_tests.rs  Commerce + order behavior tests
│   │   └── hardening_tests.rs Reporting, PII, backup, retention tests
│   └── bin/seed.rs           Seed runner binary
├── frontend/         Yew WASM SPA
│   ├── index.html            HTML shell + inline CSS
│   └── src/
│       ├── app.rs            Root component, context, token hydration
│       ├── router.rs         Client-side routes
│       ├── state.rs          AppState + UserPublic types
│       ├── api/              HTTP client wrappers (auth, store)
│       ├── components/       Nav + Layout
│       └── pages/            All UI pages
│           ├── store.rs      Product grid, cart, checkout
│           ├── orders.rs     Order history + detail
│           ├── admin_products.rs  Product management
│           ├── admin_orders.rs    Order management + dashboard
│           ├── admin_config.rs    Config values + campaigns + history
│           └── admin_kpi.rs       KPI dashboard
├── migrations/       SQLx migration files (001…015)
├── seeds/            Reference seed SQL (authoritative: bin/seed.rs)
├── exports/          Report CSV output directory (gitignored)
├── backups/          Encrypted backup storage (gitignored)
├── config/
│   └── default.env   Template environment file
└── docs/             Project documentation
```

---

## Required environment variables

| Variable | Required | Default | Notes |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | `postgres://user:pass@host:5432/dbname` |
| `SESSION_SECRET` | Yes | — | ≥ 64 characters |
| `HOST` | No | `127.0.0.1` | Bind address |
| `PORT` | No | `8080` | Bind port |
| `SESSION_MAX_AGE_SECONDS` | No | `3600` | Session lifetime |
| `LOG_LEVEL` | No | `info` | `error\|warn\|info\|debug\|trace` |
| `BACKUP_ENCRYPTION_KEY` | No | `""` | AES-256-GCM key passphrase; empty disables backups |
| `EXPORTS_DIR` | No | `../exports` | Where CSV reports are written |
| `BACKUPS_DIR` | No | `../backups` | Where encrypted `.mbak` files are stored |

---

## Prerequisites

- Rust stable (≥ 1.75) — [rustup.rs](https://rustup.rs)
- `cargo` (comes with Rust)
- `trunk` — Yew build tool: `cargo install trunk`
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- PostgreSQL 14+ running locally (no Docker required)
- `psql` CLI available
- `pg_dump` CLI available (same version as your PostgreSQL server; needed for backups)

---

## Expected startup order

1. PostgreSQL must be running and the database/user must exist.
2. Start the backend (`cargo run`). It runs all 15 migrations automatically on boot.
3. Run the seed binary once: `cargo run --bin seed`.
4. Start the frontend (`trunk serve`).

---

## Local setup (no Docker)

### 1 — Create the PostgreSQL database

```bash
psql -U postgres
```
```sql
CREATE USER meridian WITH PASSWORD 'meridian';
CREATE DATABASE meridian OWNER meridian;
GRANT ALL PRIVILEGES ON DATABASE meridian TO meridian;
\q
```

### 2 — Configure environment

```bash
cp config/default.env backend/.env
```

Edit `backend/.env` if your PostgreSQL credentials differ. To enable backups, set:

```
BACKUP_ENCRYPTION_KEY=your_strong_passphrase_here
```

Generate a strong value with:
```bash
openssl rand -hex 32
```

### 3 — Create export/backup directories

```bash
mkdir -p exports backups
```

### 4 — Start the backend

```bash
cd backend
cargo run
```

On first boot the backend will:
- Connect to PostgreSQL
- Run all 15 migrations (creates all tables, seeds config values, seeds permissions)
- Start the background scheduler (auto-close, log prune, scheduled reports)
- Serve on `http://127.0.0.1:8080`

Expected log output:
```
INFO meridian_backend: Meridian backend starting on 127.0.0.1:8080
INFO meridian_backend: Database pool created
INFO meridian_backend: Migrations applied successfully
INFO meridian_backend: Background scheduler started
INFO meridian_backend: Starting HTTP server at http://127.0.0.1:8080
INFO meridian_backend: Exports dir: ../exports  Backups dir: ../backups  Backup key: configured
```

### 5 — Run seed data

```bash
cd backend
cargo run --bin seed
```

This is idempotent — re-running it upserts rather than duplicates.

### 6 — Start the frontend

```bash
cd frontend
trunk serve --port 8081 --open
```

The frontend expects the backend at `http://localhost:8080/api/v1`.

---

## Seeded credentials

| Username | Password | Role | Notes |
|---|---|---|---|
| `admin_user` | `Admin@Meridian1!` | Administrator | `is_super_admin = true` — unrestricted access |
| `scoped_admin` | `ScopedAdmin@Meridian1!` | Administrator | Scoped to North Campus only |
| `teacher_jane` | `Teacher@Meridian1!` | Teacher | |
| `staff_carlos` | `Staff@Meridian1!` | AcademicStaff | |
| `parent_morgan` | `Parent@Meridian1!` | Parent | |
| `student_alex` | `Student@Meridian1!` | Student | |

All passwords are ≥ 12 characters and stored as Argon2id hashes.

`scoped_admin` demonstrates the scoped-by-default path: they have an explicit `admin_scope_assignments` row for North Campus and `is_super_admin = false`. Global operations (logs, backups, reports, product/config writes) return 403 for this account.

### Super-admin policy

`is_super_admin = true` is an explicit, opt-in override flag. It must be set directly in the database (or via a future provisioning workflow); no API endpoint allows self-promotion or promotion by a scoped admin. Its semantics:

- **`is_super_admin = true`** — `get_admin_campus_scope` returns `None` (unrestricted). The admin can read and act on any user, order, or deletion request regardless of campus.
- **`is_super_admin = false` (default)** — `get_admin_campus_scope` returns `Some(campus_ids)` derived from `admin_scope_assignments`. With no rows, this is `Some([])`, which blocks all object-level access (scoped-by-default, zero access until explicitly assigned).

This is an intentional architectural choice: district-level deployments need at least one unrestricted operator account for bootstrapping and cross-district operations. The flag is auditable via `SELECT id, username, is_super_admin FROM users WHERE is_super_admin = true`. Any account with this flag should be treated as a privileged credential and governed accordingly.

---

## Test commands

### Backend unit tests (no DB required)

```bash
cd backend
cargo test
```

Runs all `#[cfg(test)]` modules and the pure (non-ignored) tests in `tests/`.

### Pure integration test suites (no DB)

```bash
cd backend
cargo test --test hardening_tests
cargo test --test commerce_tests
cargo test --test admin_scope_tests
```

### DB integration tests

All DB-backed tests are tagged `#[ignore]` so the default `cargo test` run stays
fast.  They require a live PostgreSQL database and are run in CI automatically
(see `.github/workflows/ci.yml`).

**Two databases** are needed locally because `schema_integrity_tests` drops and
recreates the public schema; sharing a database with the seeded test suites would
corrupt their data.

```bash
# Spin up a throwaway Postgres container (one-time)
docker run --rm -d --name pg_test \
  --network host \
  -e POSTGRES_USER=meridian \
  -e POSTGRES_PASSWORD=meridian \
  -e POSTGRES_DB=meridian_seeded \
  -e PGPORT=5433 \
  postgres:16-alpine

# Create the second (clean-slate) database
docker exec pg_test psql -U meridian -d meridian_seeded \
  -c "CREATE DATABASE meridian_integrity;"
```

#### 1. Schema-integrity tests (clean-slate, must run isolated)

```bash
cd backend
TEST_DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_integrity?sslmode=disable" \
  cargo test --test schema_integrity_tests -- --include-ignored --test-threads=1
```

`--test-threads=1` is required: `clean_migration_succeeds` drops the schema;
parallel execution races with the other two tests in this suite.
Use `127.0.0.1` explicitly — `localhost` may resolve to `::1` (IPv6).

These tests verify:
- All 15 migrations apply without error on a blank schema
- All required tables and columns exist after migration
- Login lockout: 5 failures → 30-minute lockout row persisted in `login_lockouts`
- Lockout persists independently of the rolling attempt window
- Check-in report SQL uses `COALESCE(cad.decision, 'pending')` (not the non-existent `cs.status`)

#### 2. Apply migrations + seed to the seeded database

```bash
cd backend
DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable" \
  sqlx migrate run --source ../migrations

DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable" \
  cargo run --bin seed
```

#### 3. Seeded-DB test suites

```bash
cd backend

# PII/permission/retention integrity
TEST_DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable" \
  cargo test --test hardening_tests -- --include-ignored

# Commerce: order creation, shipping fee, points, auto-close, config history
# --test-threads=1 required: test_config_versioning mutates shipping_fee_cents
TEST_DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable" \
  cargo test --test commerce_tests -- --include-ignored --test-threads=1

# Admin scope: super-admin flag, scoped-by-default, campus isolation
TEST_DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable" \
  cargo test --test admin_scope_tests -- --include-ignored

# In-binary tests: check-in submission, filters (school_id, homeroom, date), decide
DATABASE_URL="postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable" \
  cargo test --bin meridian-backend -- --include-ignored
```

### Frontend type-check

```bash
cd frontend
cargo check --target wasm32-unknown-unknown
```

### Verified test run results (2026-04-03)

All ignored DB tests were executed against a live PostgreSQL 16 instance with
migrations 001–015 applied and seed data loaded.  Results:

| Suite | Command | Result |
|---|---|---|
| `schema_integrity_tests` | `--include-ignored --test-threads=1` | **3/3 pass** |
| `hardening_tests` | `--include-ignored` | **53/53 pass** |
| `commerce_tests` | `--include-ignored --test-threads=1` | **25/25 pass** |
| `admin_scope_tests` | `--include-ignored` | **24/24 pass** |
| binary (`meridian-backend`) | `--include-ignored` | **151/151 pass** (includes 8 admin HTTP scope tests) |

Note: `commerce_tests` requires `--test-threads=1` because `test_config_versioning`
mutates `shipping_fee_cents` and restores it; concurrent execution races with
`test_order_creation_happy_path`.  The CI workflow passes this flag.

### CI

The full suite (unit + all DB tests) runs automatically on every push and pull
request via GitHub Actions (`.github/workflows/ci.yml`).  The workflow:
1. Runs unit tests without a database.
2. Spins up a `postgres:16-alpine` service container.
3. Creates two databases (`meridian_seeded`, `meridian_integrity`).
4. Applies migrations and runs the seed binary against `meridian_seeded`.
5. Runs each test suite in the order above, with the correct `TEST_DATABASE_URL` /
   `DATABASE_URL` for each.

---

## Manual verification steps

After completing the setup steps above, save an admin token:

```bash
TOKEN=$(curl -s -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin_user","password":"Admin@Meridian1!"}' | jq -r .token)
```

---

### Core auth

**Health check:**
```bash
curl -s http://localhost:8080/api/v1/health | jq .
# Expected: {"status":"ok","version":"0.1.0"}
```

**Login:**
```bash
curl -s -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin_user","password":"Admin@Meridian1!"}' | jq .
# Expected: {"token":"<hex>","user":{...,"roles":["Administrator"],...}}
```

**Wrong password (401):**
```bash
curl -o /dev/null -w "%{http_code}\n" -s -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin_user","password":"wrong"}'
# Expected: 401
```

**No token (401):**
```bash
curl -o /dev/null -w "%{http_code}\n" -s http://localhost:8080/api/v1/auth/me
# Expected: 401
```

---

### Commerce store

**List products:**
```bash
curl -s http://localhost:8080/api/v1/products \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Get commerce config (shipping fee + points rate):**
```bash
curl -s http://localhost:8080/api/v1/config/commerce \
  -H "Authorization: Bearer $TOKEN" | jq .
# Expected: {"shipping_fee_cents":695,"shipping_fee_display":"$6.95","points_rate_per_dollar":1,...}
```

**Create an order (replace PRODUCT_ID with a real UUID from the product list):**
```bash
curl -s -X POST http://localhost:8080/api/v1/orders \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "items": [{"product_id": "PRODUCT_ID", "quantity": 1}],
    "shipping_address": "123 Main St"
  }' | jq .
# Expected: order object with status "pending", shipping_fee_cents=695, points_earned
```

**List my orders:**
```bash
curl -s http://localhost:8080/api/v1/orders \
  -H "Authorization: Bearer $TOKEN" | jq .
```

---

### Admin: order management

**Dashboard (pending/confirmed/fulfilled/cancelled + low stock):**
```bash
curl -s http://localhost:8080/api/v1/admin/orders/dashboard \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**List all orders:**
```bash
curl -s http://localhost:8080/api/v1/admin/orders \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Update order status (replace ORDER_ID):**
```bash
curl -s -X PUT http://localhost:8080/api/v1/admin/orders/ORDER_ID/status \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"status":"confirmed"}' | jq .
```

---

### Admin: KPI

```bash
curl -s http://localhost:8080/api/v1/admin/kpi \
  -H "Authorization: Bearer $TOKEN" | jq .
# Expected: daily_sales_cents, average_order_value_cents, repeat_purchase_rate_pct,
#           orders_last_30d, buyers_last_30d, repeat_buyers_last_30d
```

---

### Config management

**Update shipping fee (admin only):**
```bash
curl -s -X PUT http://localhost:8080/api/v1/admin/config/shipping_fee_cents \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"value":"895","reason":"Carrier rate increase Q2"}' | jq .
# Expected: updated config_value row
```

**Toggle campaign (e.g. free_shipping):**
```bash
curl -s -X PUT http://localhost:8080/api/v1/config/campaigns/free_shipping/status \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"enabled":true}' | jq .
```

---

### Reports and exports

**Generate a masked orders report:**
```bash
curl -s -X POST http://localhost:8080/api/v1/reports \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "report_type": "orders",
    "start_date": "2026-01-01",
    "end_date": "2026-03-31",
    "pii_masked": true
  }' | jq .
# Expected: {"id":"...","status":"completed","path":"..."}
```

**Generate an unmasked orders report (requires pii_export permission):**
```bash
curl -s -X POST http://localhost:8080/api/v1/reports \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "report_type": "orders",
    "start_date": "2026-01-01",
    "end_date": "2026-03-31",
    "pii_masked": false
  }' | jq .
# Expected: same as above, but CSV contains real emails/usernames
# (fails with 403 if user lacks pii_export permission)
```

**Range > 12 months (should fail with 400):**
```bash
curl -o /dev/null -w "%{http_code}\n" -s -X POST http://localhost:8080/api/v1/reports \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"report_type":"orders","start_date":"2024-01-01","end_date":"2026-01-03","pii_masked":true}'
# Expected: 400
```

**Non-admin report access (should fail with 403):**
```bash
STUDENT_TOKEN=$(curl -s -X POST http://localhost:8080/api/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"student_alex","password":"Student@Meridian1!"}' | jq -r .token)
curl -o /dev/null -w "%{http_code}\n" -s -X POST http://localhost:8080/api/v1/reports \
  -H "Authorization: Bearer $STUDENT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"report_type":"orders","start_date":"2026-01-01","end_date":"2026-01-31","pii_masked":true}'
# Expected: 403
```

**Download a report (replace REPORT_ID):**
```bash
curl -s http://localhost:8080/api/v1/reports/REPORT_ID/download \
  -H "Authorization: Bearer $TOKEN" \
  --output report.csv
cat report.csv
```

**List report jobs:**
```bash
curl -s http://localhost:8080/api/v1/reports \
  -H "Authorization: Bearer $TOKEN" | jq .
```

---

### Logs

**Audit log (last 100 entries):**
```bash
curl -s "http://localhost:8080/api/v1/logs/audit?limit=20" \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Access log (failures only):**
```bash
curl -s "http://localhost:8080/api/v1/logs/access?success=false&limit=20" \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Error log (errors only):**
```bash
curl -s "http://localhost:8080/api/v1/logs/errors?level=error&limit=20" \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Date-range filtering:**
```bash
curl -s "http://localhost:8080/api/v1/logs/audit?since=2026-01-01&until=2026-03-31" \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Manual log prune:**
```bash
curl -s -X POST http://localhost:8080/api/v1/logs/prune \
  -H "Authorization: Bearer $TOKEN" | jq .
# Expected: {"message":"Log retention pruning completed..."}
```

---

### Backups

> Backups require `BACKUP_ENCRYPTION_KEY` to be set in `backend/.env`.
> Requires `pg_dump` in `PATH`.

**Create a backup:**
```bash
curl -s -X POST http://localhost:8080/api/v1/backups \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"notes":"pre-deploy backup"}' | jq .
# Expected: {"backup_id":"...","filename":"backup_<uuid>.mbak","size_bytes":...,"status":"completed",...}
```

**List backups:**
```bash
curl -s http://localhost:8080/api/v1/backups \
  -H "Authorization: Bearer $TOKEN" | jq .
```

**Prepare a restore (replace BACKUP_ID):**
```bash
curl -s -X POST http://localhost:8080/api/v1/backups/BACKUP_ID/restore \
  -H "Authorization: Bearer $TOKEN" | jq .
# Expected: {"restore_path":"...","psql_command":"psql ...","warning":"The server has NOT applied this restore..."}
# The .mbak file is decrypted, integrity-verified, and written as a plain .sql file.
# Follow the returned psql_command to apply it manually.
```

**Non-admin backup access (should fail with 403):**
```bash
curl -o /dev/null -w "%{http_code}\n" -s http://localhost:8080/api/v1/backups \
  -H "Authorization: Bearer $STUDENT_TOKEN"
# Expected: 403
```

---

### Frontend flows

1. Open `http://localhost:8081` — redirected to `/login` if not authenticated.
2. Log in as `admin_user` / `Admin@Meridian1!` — Administrator dashboard with all tiles.
3. Navigate to **Store** — product grid with cart sidebar; shipping and points shown.
4. Add a product to cart and check out — creates an order; inventory decremented.
5. Navigate to **My Orders** — order history with expandable detail.
6. Navigate to **Admin → Products** — create/edit/deactivate products.
7. Navigate to **Admin → Orders** — dashboard tiles + order table + status updates; auto-refreshes every 30 seconds.
8. Navigate to **Admin → Config** — Config Values tab, Campaigns tab, Change History tab.
9. Navigate to **Admin → KPIs** — six KPI cards.
10. Log out — token cleared, redirected to login.
11. Log in as `student_alex` — Student dashboard; no admin links in nav.

---

## KPI definitions

| Metric | Definition |
|---|---|
| `daily_sales_cents` | Sum of `total_cents` for orders with `status IN ('confirmed','fulfilled')` placed today |
| `average_order_value_cents` | `daily_sales_cents / orders_today` (0 if no orders today) |
| `repeat_purchase_rate_pct` | `(repeat_buyers_last_30d / buyers_last_30d) * 100` (0 if no buyers) |
| `orders_last_30d` | Count of confirmed/fulfilled orders in the last 30 days |
| `buyers_last_30d` | Count of distinct customers with confirmed/fulfilled orders in the last 30 days |
| `repeat_buyers_last_30d` | Count of distinct customers with ≥ 2 confirmed/fulfilled orders in the last 30 days |

---

## Background scheduler behavior

The scheduler spawns once on startup and runs a loop with 60-second ticks:

| Job | Frequency | What it does |
|---|---|---|
| Auto-close unpaid orders | Every tick (60s) | Cancels `pending` orders older than 30 minutes using `FOR UPDATE SKIP LOCKED` |
| Log retention prune | Nightly at midnight | Deletes `audit_logs`, `access_logs`, `error_logs` entries older than `log_retention_days` (default 180); records the prune in `audit_logs` |
| Daily operational report | Nightly at midnight | Generates `operational` CSV for the previous day, writes to `exports/` |
| Weekly KPI report | Monday at midnight | Generates `kpi` CSV for the previous 7 days, writes to `exports/` |

The scheduler uses `last_daily` and `last_weekly` date tracking to avoid generating duplicate reports if it restarts within the same day.

---

## PII masking

| Data type | Masked form | Unmasked (requires `pii_export` permission) |
|---|---|---|
| UUID/ID | `…{last 4 chars}` e.g. `…0000` | Full UUID |
| Email | `{first char}***@{domain}` e.g. `a***@school.org` | Full email |
| Username | `{first char}***` e.g. `a***` | Full username |
| KPI/operational reports | No PII (aggregate only) | Not applicable |

PII masking is **ON by default**. To disable it, a user must:
1. Be in the `Administrator` role
2. Have the `pii_export` permission (seeded for Administrator in migration 012)
3. Explicitly set `"pii_masked": false` in the report request

---

## Backup encryption

- Algorithm: **AES-256-GCM** (authenticated encryption)
- Key derivation: SHA-256 of `BACKUP_ENCRYPTION_KEY` passphrase → 32-byte key
- File format: `MBACK01\0` (8-byte magic) + 12-byte random nonce + AES-GCM ciphertext + 16-byte authentication tag
- Checksum: SHA-256 of the **encrypted** file, stored in `backup_metadata`
- Restore: API decrypts, verifies checksum, writes `restore_{ts}.sql`; the server **never** auto-applies the SQL
- Tamper detection: AEAD tag fails if any byte of the ciphertext is modified

---

## What should work after startup

- ✅ Backend health endpoint
- ✅ Login with any seeded user (bearer token)
- ✅ `GET /api/v1/auth/me` with valid token
- ✅ Logout (invalidates session)
- ✅ Wrong password → 401; disabled account → 403
- ✅ All 20+ database tables created via migrations
- ✅ Commerce: product listing, cart, order creation with real shipping + points
- ✅ Admin: product CRUD, order management, status transitions, dashboard
- ✅ Config: live-editable shipping fee, points rate, campaign toggles with full history
- ✅ KPI: daily sales, AOV, repeat purchase rate, 30-day metrics
- ✅ Reports: checkins, approvals, orders, kpi, operational; PII masking by default
- ✅ Exports: CSV files written to `exports/` directory
- ✅ Scheduled reports: daily operational + Monday KPI (written to `exports/`)
- ✅ Backups: AES-256-GCM encrypted pg_dump; restore preparation (manual apply)
- ✅ Logs: audit, access, error log viewer with date/level filtering; 180-day retention
- ✅ Audit trail: all state-changing operations logged to `audit_logs`
- ✅ Background scheduler: auto-close unpaid orders after 30 min
- ✅ Frontend: store, orders, admin products/orders/config/kpi pages

---

## Known limitations

- `pg_dump` must be in `PATH` for backups to work (same major version as PostgreSQL server)
- No email/SMS delivery (notifications table and insert logic exist; transport not wired)
- No HTTPS (local development is plain HTTP; add a reverse proxy for production)
- No CSRF protection beyond `SameSite` policy (Bearer tokens, not cookies)
- DB integration tests require `TEST_DATABASE_URL` / `DATABASE_URL` to be set locally; they run automatically in CI
- Two test databases are required locally (`meridian_integrity` for clean-state tests, `meridian_seeded` for seeded-data tests) — see the "Test commands" section
- The frontend scheduler (auto-refresh on admin orders page) uses a 30-second `Interval`; this requires the page to remain open

---

## High-risk areas / test coverage

| Area | Coverage status |
|---|---|
| All migrations from clean state | `schema_integrity_tests::clean_migration_succeeds` — DB test, runs in CI |
| Login lockout semantics (5 attempts / 30 min) | `schema_integrity_tests::login_lockout_semantics` — DB test, runs in CI |
| Check-in report SQL (`COALESCE` fix) | `schema_integrity_tests::checkin_report_sql_uses_coalesce_not_cs_status` — DB test, runs in CI |
| AES-256-GCM encrypt/decrypt | Unit tests in `services/backup.rs` + `hardening_tests::encryption` |
| PII masking logic | Unit tests in `services/masking.rs` + `hardening_tests::pii_masking` |
| `pii_export` DB permission | `hardening_tests` DB tests — runs in CI |
| Date-range guardrail (366 days) | Pure unit tests in `hardening_tests::report_range` |
| Order creation / inventory decrement | `commerce_tests` DB test — runs in CI |
| Auto-close scheduler | `commerce_tests` DB test — runs in CI |
| Restore safety (no DROP DATABASE) | Behavioral contract test in `hardening_tests::backup_auth` |
| Log retention (180 days) | Pure unit tests; `hardening_tests` DB test for config seed — runs in CI |
| Config versioning (history) | `commerce_tests` DB test — runs in CI |
| Admin scope isolation (`is_super_admin`) | `admin_scope_tests` DB tests — runs in CI |
| Check-in school_id filter | `routes::checkins::tests::test_filter_by_school_id` — DB test, runs in CI |
| `approve_deletion` scope guard (HTTP 403) | `routes::admin::tests::test_approve_deletion_blocked_outside_scope` — DB test, runs in CI |
| `reject_deletion` scope guard (HTTP 403) | `routes::admin::tests::test_reject_deletion_blocked_outside_scope` — DB test, runs in CI |
| `list_users` scoped filtering | `routes::admin::tests::test_list_users_scoped_by_campus` — DB test, runs in CI |
| `list_deletion_requests` scoped filtering | `routes::admin::tests::test_list_deletion_requests_scoped_by_campus` — DB test, runs in CI |
| `admin_list_orders` scoped filtering | `routes::admin::tests::test_list_orders_scoped_by_campus` — DB test, runs in CI |
| Frontend (all pages) | Manual verification only — no automated frontend tests |
