# Brief coverage

Where each requirement of the product brief (`.orchestra/brief.txt`) is implemented, how to check it, and an honest status. Statuses: **done**, **partial**, **not implemented**. "Mock" means the mechanism is real but the external party is simulated inside the app — see below.

How to verify: `cargo test` runs the Rust tests named below (`cargo test <name>`), `scripts/smoke-integration.sh` drives full journeys over real HTTP, and the click paths assume the seeded demo (`servicehub seed-demo`, personas via `/demo`).

## §6 — the product, items 1–23

| § | Item | Where implemented | How to verify | Status |
|---|------|-------------------|---------------|--------|
| 1 | Personal & organisation accounts; representatives; revocation kills old links | `server/src/auth` + `records` organisations; pages `/register`, `/my/organisation` | `invitations_bind_email_and_revocation_removes_organisation_case_access`; copied-link check in DEMO-GUIDE | ✅ done |
| 2 | Understandable service catalogue; everyday-word search | `services::api` catalogue + synonyms; `/services`, `/services/:slug` | `catalogue_synonyms_and_idempotent_canonical_seed`; browse `/services` | ✅ done |
| 3 | Staff-built services (fields, docs, workflow) with draft check before publish | `services::builder` (`/api/admin/services*`); pages `/admin/services`, `/admin/services/:id` | `validates_structure_references_catalogue_and_hidden_answers`, `import_creates_only_valid_drafts_and_http_projections_deny_notes_and_published_edits`; story 7 | ✅ done |
| 4 | Bulk import of prepared service descriptions; AI suggests, never invents | `services::builder::import` (`/api/admin/service-imports`); AI helper `POST /mock/ai/suggest` (`AI_ENABLED`, off by default) | same import tests + `ai_disabled_is_404_without_affecting_catalogue`; `/admin/service-imports` | ✅ done — AI provider is a mock |
| 5 | Drafts autosave; submit once; frozen submission snapshot | `cases::api` drafts/submit; `cases::snapshot`; pages `/my/drafts/:id` | `concurrent_submission_is_one_number_one_snapshot_and_v2_cannot_change_it` | ✅ done |
| 6 | Assisted intake (phone, walk-in, letter) without an account | `cases::api` assisted intake (`/api/staff/intake`); page `/staff/intake` | integration smoke intake journey; demo guide extra scene | ✅ done |
| 7 | Assignment, collaborators, replacement/escalation, hand-over history | `cases::api` assign/collaborators; staff case page team panel | `step_roles_revisions_and_internal_projection`; case → assign staff | ✅ done |
| 8 | Per-document comments, kept versions, internal notes vs applicant messages | `documents::api` comments/versions; `cases` messages vs internal notes (separate projections) | `immutable_versions_visibility_revocation_and_exact_evidence`, `replacement_resolves_only_earlier_comments_on_same_document_and_preserves_source`, `import_creates_only_valid_drafts_and_http_projections_deny_notes_and_published_edits` | ✅ done |
| 9 | Decisions prepared → issued by authorised officer; refusal recorded; sysadmin ≠ authority | `documents::decisions` + `authz` authority grants; Decisions tab | `authority_is_explicit_scoped_and_revocable_and_seeds_idempotent`, `issued_pdf_pins_evidence_refusals_and_separate_approvals`, `inactive_authority_does_not_rollback_decision_submission`, `closed_case_cannot_issue_pending_decision_or_create_artifact` | ✅ done |
| 10 | Building project history: linked application, approvals, modification, notices | `documents::projects`; `/my/projects/:id`, staff project page | `project_creation_modification_links_and_supersession_reference_are_stable`; story 2 | ✅ done |
| 11 | Public exhibition with true redaction; public submissions in a window | `documents::exhibitions` + `redaction`; `/staff/exhibitions`, `/notices/:id` | `redaction_has_no_text_layer_and_source_remains_unchanged`, `publishing_uses_new_blob_with_burned_pixels_and_independent_staff_approval`, `public_submission_window_and_public_projection_never_expose_source` | ✅ done |
| 12 | Room calendar incl. buffers/maintenance; incompatible bookings can't both confirm; reschedule/cancel change availability | `operations::bookings`, `resources`; Booking tab, `/staff/calendar`, `/admin/resources` | `concurrent_whole_and_main_confirmation_exactly_one_wins`, `failed_reschedule_keeps_original_occupancy_and_history`, `reschedule_rejects_consumed_original_booking_before_mutation`, `maintenance_conflict_hides_case_number_and_label_without_access`, `withdrawn_case_releases_booking_and_equipment_and_cancels_open_tasks` | ✅ done |
| 13 | Equipment: assigned plant + operator; actual time/downtime/expenses → final invoice; estimate stays separate | `operations::equipment` + `tasks`; Equipment tab, `/staff/field` | `norfolk_hours_midnight_and_exact_usage`; equipment journey in `smoke-integration.sh` | ✅ done |
| 14 | Fixed & hourly prices, deposits, exclusions; scheduled future rates; old invoices keep old rates | `finance::prices` (`/api/admin/prices`); `/admin/prices` | `old_invoice_keeps_old_rate_and_generic_snapshot_quantity`, `zero_rated_booking_still_requires_an_issued_invoice`; `/admin/prices` | ✅ done |
| 15 | Online payment via provider confirmation, bank statement import & matching; partial/over-payment; dedupe | `finance::payments` (DemoPay webhook), `statements`, `unmatched`; Money tab, `/staff/finance/*` | `duplicate_webhook_delivery_is_idempotent`, `invalid_signature_stores_nothing_and_cannot_poison_money`, `statement_file_and_transaction_deduplication`, `ambiguous_reference_and_partial_manual_match`, `reallocation_keeps_history_and_credit_note_settles_old_invoice`, `webhook_rejections_are_size_rate_and_concurrency_limited_without_storage` | ✅ done — provider is the built-in DemoPay mock |
| 16 | Bond held separately; recorded refund/retention decision with reasons; refund completes only on provider/bank confirmation | `finance::deposits`, `refunds`; Money tab, `/staff/finance/deposits`, `/staff/finance/refunds` | `deposit_settlement_truth_table_and_bank_confirmation`, `fully_retained_bond_needs_no_refund`, `concurrent_refunds_can_never_exceed_paid`, `provider_refund_failure_preserves_liability_then_completion`, `cancelled_unused_paid_booking_bond_refunds_without_inspection`, `demopay_redirect_does_not_pay_async_refund_and_delivery_retry` | ✅ done |
| 17 | Field tasks on a phone: checklist, photos, result; offline draft survives and confirms after sync | `operations::tasks`; `/staff/field`, `/staff/field/:id` (responsive UI + local draft queue + "Simulate offline") | `task_projection_idempotency_stale_revision_and_run_isolation`; offline steps in DEMO-GUIDE | ✅ done — offline queue is a local draft store, not a service worker |
| 18 | Confidential complaints: separate owner, access limits, linked reviews, history kept | `records::complaints` + `case_access_denials`; Confidential feedback panel, `/staff/authority` | `subject_endpoint_excludes_even_managers_from_read_search_count_and_export`, `finance_queues_obey_case_denials_and_confidentiality`; story 6 + safe-fail checks | ✅ done |
| 19 | Per-service deadlines; business vs calendar days; capped pauses while waiting on the applicant; completeness ≠ final clock | `cases::deadlines` + scheduler sweep | `request_info_pauses_only_pausable_clock_and_reply_extends_business_days`, `delayed_sweep_caps_pause_and_breach_is_notified_once`, `calendar_deadline_rolls_forward_and_late_reply_cannot_exceed_cumulative_cap` | ✅ done |
| 20 | Finished-case archive: find by person/object/number; per-class retention; legal hold blocks disposal | `records::retention`, `search`, `disposal`; `/staff/records`, `/admin/retention` | `legal_hold_blocks_disposal_and_only_manager_can_grant_authority`, `disposal_endpoint_preserves_decision_evidence_and_shared_blob_until_last_consumer`, `reclose_extends_retention_and_delivers_distinct_closure_with_stable_retries` | ✅ done |
| 21 | Manager dashboard: received/open/closed, lateness, workload; every number drills to its cases; cancelled/reopened counted separately | `records::dashboard`; `/staff/dashboard`, `/staff/dashboard/metrics/:metric` | `every_metric_count_matches_drilldown_and_denials` | ✅ done |
| 22 | Legacy import with duplicate detection; full case export; outbound integration with visible errors and safe retry | `records::legacy`, `export`, `integrations` (outbox); `/admin/legacy-import`, `/admin/integrations`, `/admin/deliveries` | `legacy_preview_reports_bad_rows_and_both_duplicate_kinds`, `outbox_outage_and_lost_response_are_recoverable_without_duplicates`, `integration_delivery_receipt_does_not_invalidate_staff_revision`, `distinct_closure_generations_have_distinct_delivery_keys` | 🔶 **partial** — mechanism done and tested; receivers are the built-in Content Manager / Civica Altitude mocks. Real NIRC connectivity can't be verified without access (the brief itself allows this) |
| 23 | Admin self-manages users, services, prices, notification addresses; sees delivery errors & backup state; another developer can install from docs | `admin` pages `/admin/users`, `/admin/services`, `/admin/prices`, `/admin/settings`, `/admin/deliveries`, `/admin/backups`; `deploy/README.md` + `docs/OPERATIONS.md` | click through `/admin/*`; `backup_roundtrip_and_tamper_detection`; install steps in OPERATIONS | ✅ done |

## §7 — readiness checks

| Check in the brief | Covered by | Status |
|---|---|---|
| Create a brand-new service via UI, walk it end-to-end as a new resident, then edit the form — old submission keeps its fields | Builder tests + `concurrent_submission_is_one_number_one_snapshot_and_v2_cannot_change_it`; presenter story 7 | ✅ covered |
| Building case: corrected drawing, separate decisions, linked follow-up; public copy leaks nothing | `issued_pdf_pins_evidence…`, `publishing_uses_new_blob_with_burned_pixels…`, `public_submission_window…`, `project_creation_modification_links…`; story 2 | ✅ covered |
| Certificate after the required check; hall booking → move → partial bond return; equipment billed on actual time | `planning_certificate_reissue_retains_old_result_and_requested_sections`; booking/deposit/refund tests above; `norfolk_hours_midnight_and_exact_usage`; stories 1, 3, 4 | ✅ covered |
| Simultaneous whole-hall + room booking; duplicated payment confirmation; unclear transfer | `concurrent_whole_and_main_confirmation_exactly_one_wins`, `duplicate_webhook_delivery_is_idempotent`, `ambiguous_reference_and_partial_manual_match`; safe-fail steps in DEMO-GUIDE | ✅ covered |
| Phone request to an answer without an account; complaint about a staff member — who sees it; copied document link | intake journey; `subject_endpoint_excludes_even_managers_from_read_search_count_and_export`; download recheck (safe-fail steps) | ✅ covered |
| Kill the external program mid-exchange → retry makes no duplicate; export a case; restore on another server; dashboard numbers match reality | `outbox_outage_and_lost_response_are_recoverable_without_duplicates`, `backup_roundtrip_and_tamper_detection`, `every_metric_count_matches_drilldown_and_denials`; restore procedure in OPERATIONS | ✅ covered |

## §8 — build & hand-over requirements

| Requirement | Where / evidence | Status |
|---|---|---|
| One self-contained web application (not scattered demos) | single `servicehub` binary + one SQLite DB + blob dir | ✅ done |
| English UI | `web/` | ✅ done |
| Staff change fields/rules via settings, not code | `/admin/services` builder, `/admin/prices`, `/admin/settings` | ✅ done |
| Phone, keyboard, text zoom, clear error messages | responsive layouts, focus-visible styles, `aria-live` regions | ✅ done — automated axe checks are part of the planned e2e suite, not yet run in CI |
| Confidential data protected **on the server**, including search and downloads | `authz` projection enforcing denials; tests `subject_endpoint_excludes…`, `maintenance_conflict_hides…`, `public_road_projection_never_contains_reporter_text_or_names` | ✅ done |
| Second factor for staff | TOTP enrolment; `demo_staff_login_requires_totp_and_rejects_replay` | ✅ done |
| Uploaded files validated | `rejected_uploads_register_nothing`, `aggregate_upload_quota_rejects_attachment_without_registering_more_bytes`, `enormous_pdf_geometry_is_rendered_with_bounded_dimensions` | ✅ done |
| No personal documents/payment secrets/keys in git | only `.env.example` placeholders; secrets are generated per deploy | ✅ done |
| Amounts & deadlines by rules, not LLM | money = integer cents + ledger; deadlines = `cases::deadlines` | ✅ done |
| AI may draft, human checks; switching AI off stops nothing | `AI_ENABLED` off by default; `ai_disabled_is_404_without_affecting_catalogue` | ✅ done — mock helper only |
| Install/config/seed/tests/upgrade/backup/verified-restore in the package | `deploy/README.md`, `docs/OPERATIONS.md`, `servicehub backup` / `restore-check`; `backup_roundtrip_and_tamper_detection` | ✅ done |
| "Connect to existing programs" | outbox + mock receivers (`content_manager`, `civica_altitude`) | 🔶 partial — real NIRC systems can't be verified without access; the brief explicitly permits this |

## §9 — showing it to the administration

| Requirement | Where | Status |
|---|---|---|
| Fictional residents & staff; checker can drive the story themselves | seeded personas; `/demo` one-click sign-in | ✅ done |
| Show: new service via settings, building with corrections, hall with move+refund, equipment actual billing, confidential complaint | DEMO-GUIDE stories 7, 2, 1, 4, 6 | ✅ done |
| Results and summaries come from those actions, not drawn | everything above is produced by live requests | ✅ done |
| No real submissions/documents; payments in provider test mode with a real technical cycle | fictional seed; DemoPay checkout + signed webhooks | ✅ done |
| Code stays open; only the hosted demo switches off after the period; council can self-host with no timer/subscription/hidden access | MIT licence; `DEMO_ENDS_AT` ended-page; `DEMO_MODE=false` self-hosting | ✅ done |
| Cooperation offer: implementation, migration, training; free code ≠ promised free support | README "Cooperation" | ✅ done |
| Pre-seeded history (past bookings, project, certs, road issues, complaint + review) so every screen has content immediately | `server/src/seed/scenarios.rs` | 🚧 **in progress** — the scenario seeder is being filled by the current work stream; until it lands, stories can be performed live from empty state |

## Honest gaps

- **Real external integrations (§22, §8): partial by design.** The outbox, signed delivery, receipts, failure visibility and idempotent retry are implemented and tested — against the built-in mock receivers. Nothing claims a verified connection to Council's actual Content Manager or Civica Altitude.
- **Email/SMS are DemoMail** — a real outbox with dedupe/retries delivering to an in-app mailbox. Real delivery needs a provider adapter.
- **Pre-seeded demo history** is the remaining open item; check `server/src/seed/scenarios.rs` and the `e2e/` suite for current status.
- **Automated accessibility testing** (axe-core) is planned with the e2e suite; the UI implements the practices but no automated audit has run yet.
