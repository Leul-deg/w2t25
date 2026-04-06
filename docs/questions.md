Business Logic Questions Log:

Check-In Window Scope
Question: The prompt did not specify if check-in windows should be district-wide, school-specific, class-specific, or all three depending on context.
My Understanding: Windows are primarily school-scoped, with class linkage available where needed.
Solution: Configured check-in windows to be primarily school-scoped, with options for class linkage where needed.

Parent Submission Scope
Question: The prompt did not clarify whether a parent can submit for multiple linked students within a single day and single UI view.
My Understanding: Yes, but only for explicitly linked students.
Solution: Enabled multi-student submission support within a single UI view for parents with explicitly linked students.

Order Payment Semantics
Question: The prompt mentioned unpaid order auto-close after 30 minutes, but did not specify if this implies a future local payment status model, or if `pending` is enough.
My Understanding: `pending` represents created-but-not-completed payment state for local/offline operation.
Solution: Used `pending` as the state for created-but-not-completed payments to simplify local/offline operation context.

Inventory Reservation Timing
Question: The prompt did not specify whether stock should be reserved at order creation or only after confirmation.
My Understanding: Stock is decremented at order creation for operational simplicity in offline mode.
Solution: Decremented stock at order creation to ensure operational simplicity when operating offline.

Admin Scope Rules
Question: If an admin is campus- or district-scoped, the prompt did not specify if they should be able to manage only users/orders/products within those assigned scopes.
My Understanding: Yes. Admin scope should constrain visible and mutable records where scope assignments exist.
Solution: Implemented scope-based filtering to constrain visible and mutable records for admins based on their assigned campus or district scopes.

Report Scope
Question: The prompt did not state whether report generation should be admin-only, or if academic staff should have limited reporting access later.
My Understanding: Required to be Admin-only for now, since exports and console controls fall under administrator operations.
Solution: Restricted report generation access strictly to admins.

PII Masking Granularity
Question: Beyond IDs and usernames, the prompt did not specify if all person-facing fields such as email, display name, and addresses should be masked in all exports.
My Understanding: Any identifying data should be masked by default unless explicit `pii_export` permission overrides it.
Solution: Applied default masking to all identifying data in exports, requiring explicit `pii_export` permission to override.

Restore Execution Model
Question: The prompt did not indicate whether the service should ever apply restore SQL automatically.
My Understanding: No. Restore should remain a controlled manual action after the system prepares and verifies the restore artifact.
Solution: Ensured that system restoration remains a controlled, manual process following artifact preparation and verification.

Notifications
Question: The prompt did not list if there are any notification types that should be treated as critical beyond security or system alerts.
My Understanding: Only narrow alert/system events bypass DND and frequency deferral.
Solution: Configured only narrow alert and system events to bypass Do Not Disturb (DND) and frequency deferrals.

Multi-Campus Product Scope
Question: The prompt did not explain if products are global across the district or scoped to campus/school.
My Understanding: Global by default unless later product scoping requirements are introduced.
Solution: Treated products as global by default, with room for future scoping configurations if needed.

Restore Preparation for Delivery
Question: The prompt did not specify if manual restore preparation is acceptable for delivery, or if a fully automated restore is required.
My Understanding: Manual restore preparation is acceptable as a baseline for delivery.
Solution: Prepared the system to support manual restore preparations for the initial delivery phase.

Admin Order Dashboard Real-Time Behavior
Question: The prompt did not clarify if polling is acceptable for the admin order dashboard, or if websocket-style real-time behavior is required.
My Understanding: Polling is acceptable for offline-first design constraints, rather than complex websocket setups.
Solution: Implemented polling for the admin order dashboard to update order statuses.

Sample Data Sufficiency
Question: The prompt did not specify if current seeded roles and sample data are sufficient for local review.
My Understanding: Current seeded roles and data provide an adequate representation for review.
Solution: Provided seeded roles and sample data to facilitate local testing and review.

Dashboard Navigation Links
Question: The prompt did not ask whether admin reporting, logs, and backups should be linked directly from the home dashboard and nav for review friendliness.
My Understanding: Direct links improve the review experience by making administrative operations easily accessible.
Solution: Added direct links for reporting, logs, and backups to the home dashboard and navigation menu.
