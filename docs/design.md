# Meridian Check-In & Commerce Operations Suite

## Overview

Meridian is an offline-first school district operations platform that combines:

- daily student/parent check-in
- teacher and academic staff review workflows
- an in-app notification and inbox system
- a merchandise store
- administrative operations, KPI dashboards, exports, backups, and auditability

The system is designed for local-only operation over `localhost` or a local network, with all data stored in PostgreSQL and no dependency on cloud authentication or third-party messaging.

## Product Goals

- Make daily student attendance check-in fast and low-friction.
- Give teachers and academic staff a scoped review queue with clear approval and denial behavior.
- Provide administrators with a practical operations console for users, orders, products, configuration, KPIs, exports, logs, backups, and restore preparation.
- Preserve auditability, offline operation, and strong local security controls.

## Primary Roles

- `Student`
- `Parent`
- `Teacher`
- `AcademicStaff`
- `Administrator`

## Core User Flows

### Student / Parent

- Sign in with local username and password.
- View today's check-in window and its current status.
- Perform a one-tap check-in.
- Receive immediate on-screen confirmation.
- See reminders for upcoming deadlines or missed check-ins.
- Review approval outcomes in an in-app inbox.
- Manage notification preferences, including subscription toggles, Do Not Disturb, and inbox frequency.
- Browse store products, place orders, and review order history.

### Teacher / Academic Staff

- Access scoped check-in review screens.
- Filter by school, homeroom, and date range.
- Approve submissions.
- Deny submissions with a required reason.
- Operate only within assigned data scope.

### Administrator

- Manage users, account states, blacklist rules, and deletion approvals.
- Manage products, inventory, orders, and configuration values.
- Monitor a near-real-time order dashboard and KPI views.
- Generate reports and exports with PII masking by default.
- Create encrypted backups and prepare manual restore operations.
- Review audit, access, and error logs.

## Architecture

### Frontend

- Framework: `Yew`
- Delivery: browser-based SPA
- Responsibilities:
  - role-based navigation and route guards
  - check-in and review workflows
  - inbox and preferences
  - merch store and order history
  - admin console pages
  - quick-lock on sensitive screens

### Backend

- Framework: `Actix-web`
- Style: decoupled REST-style API
- Responsibilities:
  - authentication and session validation
  - RBAC and scope enforcement
  - check-in workflow rules
  - notifications and reminders
  - commerce, inventory, and KPI calculations
  - report generation and export writing
  - encrypted backup creation and restore preparation
  - logging, auditing, and scheduled jobs

### Database

- Engine: `PostgreSQL`
- Responsibilities:
  - source of truth for users, roles, school hierarchy, check-ins, notifications, products, orders, config, logs, jobs, and backup/report metadata
  - indexed date and scope fields for reporting and filtering

## Security Design

- Local-only authentication.
- Password minimum: 12 characters.
- Password storage: salted Argon2 hashes.
- Login protection: 5 attempts per username per 15 minutes with 30-minute lockout.
- No CAPTCHA, SMS, email auth, OAuth, or third-party identity providers.
- RBAC enforced at:
  - menu level
  - route/API level
  - data-scope level
- Account states:
  - active
  - disabled
  - frozen
  - blacklisted
- Account deletion requests require administrator review.
- Sensitive screens support quick-lock after 10 minutes of inactivity.
- Exports mask PII by default.
- Unmasked exports require explicit `pii_export` permission.
- Backups are encrypted with AES-256 using a locally managed key.

## Major Domain Areas

### Organization and Identity

- districts
- campuses
- schools
- classes / homerooms
- users
- roles and permissions
- user-to-school and parent-to-student relationships

### Check-In Domain

- check-in windows
- check-in submissions
- approval decisions
- reminders and inbox notifications
- reviewer scope and denial reason enforcement

### Commerce Domain

- products
- inventory
- orders and order items
- shipping fee config
- points earning config
- low-stock alerts

### Operations Domain

- configuration values and change history
- campaign toggles
- KPI metrics
- report jobs
- backup metadata
- audit, access, and error logs

## Scheduled Jobs

- auto-close unpaid `pending` orders after 30 minutes
- generate low-stock alerts when stock drops below threshold
- generate scheduled daily reports
- generate scheduled weekly reports
- prune logs older than the configured retention period

## Reporting and Export Design

- Export destination: local export folder
- Supported report families:
  - check-ins
  - approvals / denials
  - orders
  - KPI
  - operational summaries
- Date range limit: default maximum of 12 months per request
- PII masking:
  - IDs show last 4 characters only
  - usernames and other identifiers are masked by default

## Backup and Restore Design

- Backups are generated locally from PostgreSQL data.
- Backup files are encrypted before storage.
- Restore is a controlled admin flow that prepares a restore artifact and manual command.
- The running service should not silently perform destructive database replacement.

## Non-Functional Requirements

- offline-first
- local-only deployment
- auditable changes and operations
- reviewer-friendly startup and verification flow
- maintainable module boundaries
- practical admin UX over ornamental UI

## Implementation Boundary

This design assumes a single deployable product composed of:

- `repo/` for source code
- `docs/` for project documentation
- `sessions/` for converted AI development trajectories
- `raw_sessions/` for raw development session exports

The system is a full-stack application with Rust on both frontend and backend and PostgreSQL as the persistence layer.
