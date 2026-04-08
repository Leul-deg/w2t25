# Fixed Issues And How They Were Fixed

Based on the progression from `self-report-review-1.md` through `self-report-review-05.md`.

- **Migration/schema mismatch**
  - Original issue: the first audit reported a blocker because `012_hardening.sql` indexed `checkin_submissions(created_at)` while the schema used `submitted_at`.
  - How it was fixed: later reviews explicitly confirmed the schema/report mismatch was corrected so migrations and code aligned.

- **Login lockout requirement**
  - Original issue: the first audit reported missing persistent `5 attempts / 15 min + 30 min lockout` behavior.
  - How it was fixed: a persisted lockout table and enforcement logic were added, and later reviews graded authentication as `Pass`.

- **Check-in report SQL mismatch**
  - Original issue: the first audit reported `cs.status` being queried even though `checkin_submissions` had no `status` column.
  - How it was fixed: later reviews confirmed the reporting SQL/schema contract was repaired.

- **Administrator district/campus scoping**
  - Original issue: the first audit reported incomplete tenant/campus scope isolation for admins.
  - How it was fixed: a scope model and scope-aware checks were introduced, and later reviews eventually graded tenant/user isolation as `Pass`.

- **Reviewer filtering gap**
  - Original issue: the first audit reported missing school/homeroom/date-range reviewer filtering.
  - How it was fixed: later reviews confirmed filtering support was materially improved, especially around homeroom/date/decision handling.
