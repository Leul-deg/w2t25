# Meridian API Specification

## API Style

- Base path: `/api/v1`
- Protocol: HTTP over `localhost` or local network
- Auth: Bearer token session authentication
- Response style: JSON for API responses, CSV/plain file content for report download

## Authentication

### `POST /api/v1/auth/login`

Authenticates a local user with username and password.

Request:

```json
{
  "username": "admin_user",
  "password": "Admin@Meridian1!"
}
```

Response:

```json
{
  "token": "session-token",
  "user": {
    "id": "uuid",
    "username": "admin_user",
    "roles": ["Administrator"]
  }
}
```

### `POST /api/v1/auth/logout`

Invalidates the current session.

### `GET /api/v1/auth/me`

Returns the currently authenticated user.

### `POST /api/v1/auth/verify`

Re-prompts for password verification for quick-lock unlock flow.

## Health

### `GET /api/v1/health`

Returns service liveness information.

## Users

### `GET /api/v1/users/me`

Current user profile.

### `POST /api/v1/users/me/request-deletion`

Submit account deletion request.

### `GET /api/v1/users/me/linked-students`

For parent users, returns linked student records.

## Check-Ins

### `GET /api/v1/check-ins/windows`

Returns visible check-in windows for the caller.

### `GET /api/v1/check-ins/windows/{window_id}`

Get one check-in window.

### `POST /api/v1/check-ins/windows/{window_id}/submit`

Submit a student or parent check-in.

### `GET /api/v1/check-ins/my`

Returns student or parent check-in history.

### `GET /api/v1/check-ins/windows/{window_id}/submissions`

Teacher, academic staff, or admin review list for a specific window.

### `POST /api/v1/check-ins/windows/{window_id}/submissions/{submission_id}/decide`

Approve or reject a submission. Rejections require a reason.

## Notifications and Preferences

### `GET /api/v1/notifications`

Returns current inbox items visible to the caller.

### `GET /api/v1/notifications/unread-count`

Unread inbox count.

### `POST /api/v1/notifications/{id}/read`

Mark a notification as read.

### `POST /api/v1/notifications/reminders/generate`

Generates reminder notifications for current user or linked student context.

### `GET /api/v1/preferences`

Returns current notification and inbox preferences.

### `PATCH /api/v1/preferences`

Updates notification toggles, DND, and inbox frequency.

## Public Store

### `GET /api/v1/products`

Returns active products with stock information.

### `GET /api/v1/products/{id}`

Returns one active product.

### `POST /api/v1/orders`

Creates an order from cart items.

### `GET /api/v1/orders`

Returns authenticated user's order summaries.

### `GET /api/v1/orders/{id}`

Returns authenticated user's order detail.

### `GET /api/v1/config/commerce`

Returns store-facing commerce config summary:

- shipping fee
- points rate
- campaign toggle status

### `GET /api/v1/config/campaigns/{name}/status`

Returns public campaign enabled/disabled status.

## Administrator APIs

### User and Account Operations

- `GET /api/v1/admin/users`
- `POST /api/v1/admin/users/{user_id}/set-state`
- `GET /api/v1/admin/deletion-requests`
- `POST /api/v1/admin/deletion-requests/{request_id}/approve`
- `POST /api/v1/admin/deletion-requests/{request_id}/reject`

### Product Operations

- `GET /api/v1/admin/products`
- `POST /api/v1/admin/products`
- `POST /api/v1/admin/products/{id}/update`
- `POST /api/v1/admin/products/{id}/deactivate`

### Order Operations

- `GET /api/v1/admin/orders`
- `GET /api/v1/admin/orders/dashboard`
- `GET /api/v1/admin/orders/{id}`
- `POST /api/v1/admin/orders/{id}/status`

### KPI

- `GET /api/v1/admin/kpi`

### Configuration Center

- `GET /api/v1/admin/config`
- `POST /api/v1/admin/config/values/{key}`
- `GET /api/v1/admin/config/history`
- `GET /api/v1/admin/config/campaigns`
- `POST /api/v1/admin/config/campaigns/{name}`

## Reports and Exports

### `POST /api/v1/reports`

Creates and generates a report export.

Request:

```json
{
  "report_type": "orders",
  "start_date": "2026-03-01",
  "end_date": "2026-03-31",
  "pii_masked": true
}
```

Supported report types:

- `checkins`
- `approvals`
- `orders`
- `kpi`
- `operational`

Rules:

- maximum range: 12 months
- `pii_masked` defaults to `true`
- `pii_masked: false` requires `pii_export` permission

### `GET /api/v1/reports`

List recent report jobs.

### `GET /api/v1/reports/{id}`

Get report job metadata.

### `GET /api/v1/reports/{id}/download`

Download generated CSV for completed reports.

## Logs

### `GET /api/v1/logs/audit`

Business audit log.

### `GET /api/v1/logs/access`

Access log, including login and sensitive actions.

### `GET /api/v1/logs/errors`

Application error log.

### `POST /api/v1/logs/prune`

Trigger log retention pruning.

## Backups

### `GET /api/v1/backups`

List backup metadata.

### `POST /api/v1/backups`

Create an encrypted backup.

Request:

```json
{
  "notes": "before monthly review"
}
```

### `GET /api/v1/backups/{id}`

Get backup metadata entry.

### `POST /api/v1/backups/{id}/restore`

Prepare a restore artifact and return manual restore command guidance.

## Error Behavior

Common response shapes:

```json
{
  "error": "Human-readable message"
}
```

Common status codes:

- `401` unauthenticated
- `403` forbidden
- `404` missing resource
- `409` conflict
- `422` validation failure
- `429` rate limit / lockout

## Authorization Model

Authorization is enforced at:

- menu visibility
- frontend route guards
- backend endpoint guards
- backend data-scope checks

Examples:

- Students and parents can access store and personal check-in flows.
- Teachers and academic staff can review only scoped check-ins.
- Administrators can access admin operations, reports, logs, and backups.
- Unmasked exports require explicit `pii_export` permission even for admin role.
