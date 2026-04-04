# Meridian Project Questions

This file captures project questions, assumptions, and items that may need clarification later.

## Assumptions Already Locked By Prompt

- Frontend framework is `Yew`.
- Backend framework is `Actix-web`.
- Database is `PostgreSQL`.
- Authentication is local-only.
- Offline-first behavior is required.
- Reporting, exports, backups, restore preparation, logging, and auditability are in scope.

## Likely Clarifications

### 1. Check-In Window Scope

Question:

- Should check-in windows be district-wide, school-specific, class-specific, or all three depending on context?

Current assumption:

- Windows are primarily school-scoped, with class linkage available where needed.

### 2. Parent Submission Scope

Question:

- Can a parent submit for multiple linked students within a single day and single UI view?

Current assumption:

- Yes, but only for explicitly linked students.

### 3. Order Payment Semantics

Question:

- Does "unpaid order auto-close after 30 minutes" imply a future local payment status model, or is `pending` enough for now?

Current assumption:

- `pending` represents created-but-not-completed payment state for local/offline operation.

### 4. Inventory Reservation Timing

Question:

- Should stock be reserved at order creation or only after confirmation?

Current assumption:

- Stock is decremented at order creation for operational simplicity in offline mode.

### 5. Admin Scope Rules

Question:

- If an admin is campus- or district-scoped, should they be able to manage only users/orders/products within those assigned scopes?

Current assumption:

- Yes. Admin scope should constrain visible and mutable records where scope assignments exist.

### 6. Report Scope

Question:

- Should report generation be admin-only, or should academic staff have limited reporting access later?

Current assumption:

- Admin-only for now, since the prompt describes exports and console controls under administrator operations.

### 7. PII Masking Granularity

Question:

- Beyond IDs and usernames, should all person-facing fields such as email, display name, and addresses be masked in all exports?

Current assumption:

- Any identifying data should be masked by default unless explicit `pii_export` permission overrides it.

### 8. Restore Execution Model

Question:

- Should the service ever apply restore SQL automatically?

Current assumption:

- No. Restore should remain a controlled manual action after the system prepares and verifies the restore artifact.

### 9. Notifications

Question:

- Are there any notification types that should be treated as critical beyond security or system alerts?

Current assumption:

- Only narrow alert/system events bypass DND and frequency deferral.

### 10. Multi-Campus Product Scope

Question:

- Are products global across the district or scoped to campus/school?

Current assumption:

- Global by default unless later product scoping requirements are introduced.

## Review and Delivery Questions

- Is manual restore preparation acceptable for delivery, or is a fully automated restore required?
- Is polling acceptable for the admin order dashboard, or is websocket-style real-time behavior required?
- Are current seeded roles and sample data sufficient for local review?
- Should admin reporting, logs, and backups be linked directly from the home dashboard and nav for review friendliness?

## Suggested Next Clarifications If Needed

- exact scope rules for campus/district admins
- product scoping rules
- whether academic staff should gain limited reporting access
- whether payment states need more detail than `pending`, `confirmed`, `fulfilled`, `cancelled`, and `refunded`
