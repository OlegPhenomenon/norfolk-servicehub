# Acceptance review fixes — pass 2

Branch: `integration`. No commit made. Reviewed `.orchestra/out/coverage-review.md` against current code, including the intervening UI polish. Findings below follow the review's order (P1 first). Behavioral regressions are in `server/tests/coverage_fixes.rs` unless another file is named. Migration `0701_coverage.sql` preserves old rejections and relabels previously unchecked uploads, without rewriting earlier migrations.

## Finding → fix → regression

| # / priority | Finding | Fix | Test |
|---|---|---|---|
| 1 P1 | Live bond unreachable | Alexey has a past, inspected Rawson booking with paid fee/bond and no decision. Future-event guard retained; guide separates future booking from live past bond. | `acceptance_seeded::seeded_history_is_complete_balanced_repeatable_and_leaves_the_visitor_story_free`; browser `Tom records Alexey past inspected bond and sees live partial refund complete` |
| 2 P1 | Generic result and invalid decision handlers | Any module may issue `service_response` decision/letter. Module capability API drives builder options and server validation of handlers, tasks and decision types. Seeded generic result services include a decision. | `generic_result_shared_capabilities_reject_invalid_types_and_handlers` |
| 3 P1 | Complaint subject absent/assigned too early | Staff-name select; record subject and denial before handler selection; officer-subject routes to eligible manager; managers may triage subjects. | `complaint_subject_excluded_before_routing_including_only_officer_and_manager` |
| 4 P1 | Self-approved decisions/refusal bypass | Reject approver=preparer. Helen holds independent authority for building/certificate and generic response. Decision-bearing workflows cannot use case-level refuse. | `independent_decisions_and_role_escalation_are_enforced`; generic-result regression; existing building/certificate journeys now issue as Helen |
| 5 P1 | Sysadmin role escalation | No self role grants. Manager/complaints-officer grants require manager. UI hides unavailable grants. | `independent_decisions_and_role_escalation_are_enforced` |
| 6 P1 | Mocks/checkout active outside demo | Conditionally mount all mock routes; checkout/webhook/refund jobs gated. Online Pay hidden, clear unavailable error; counter/bank paths retained. Mock AI hidden outside demo. | `bootstrap_catalogue_non_demo_mocks_and_cross_directory_restore` |
| 7 P1 | No initial administrator | `create-admin --email --name` prints random one-time password, forces TOTP enrolment/password replacement. First admin has manager to establish governance. `seed-catalogue` has no personas/cases. | Bootstrap regression; `account_recovery_forces_totp_and_password_rotation` |
| 8 P1 | Stranded fee/credit refunds | Reserve and refund available customer credit through shared refund flow. Unused booking cancellation reverses fee allocations and creates credit note; fee and bond can both be refunded. | `unused_cancellation_refunds_fee_credit_and_bond_through_live_provider` (also bounded reservation and balanced journal) |
| 9 P2 | Wrong-applicant bank match irreversible | Unmatch reverses allocations into suspense and clears case; rematch to another applicant uses distinct journal keys and preserves history. Former applicant cannot see reassigned payment balances. | `wrong_applicant_bank_match_can_return_to_suspense_and_rematch` |
| 10 P2 | Ordinary multiword search fails | Per-token synonyms, stop-words, OR matching, FTS bm25 ranking; editable synonym table/API/UI; defaults reseeded after demo resets. Case-number search retains precise AND matching. | `receipt_type_check_search_deadlines_team_copies_and_organisation_creation`; existing exact-number acceptance journeys |
| 11 P2 | Completeness described as final reply | Shared deadline text uses label/status; met means completed, paused has cap context; start/completion timeline names the target. | Receipt/search/deadline regression plus existing pause/holiday tests |
| 12 P2 | Docker WEB_DIST wrong | Example local override commented; compose pins `/app/web`. README instructions corrected. | `docker_web_dist_cannot_be_overridden_by_native_example`; web build |
| 13 P2 | Hosted first start unseeded | Fresh demo `serve` migrates and seeds once only when no users/cases; populated data preserved. | Bootstrap regression verifies fresh seed and repeat no-op |
| 14 P2 | Notification address settings unused | `notify::send` creates nonconfidential staff team copies using configured Care/Finance/Works addresses. Field notifications now carry case ID for confidentiality/access rechecks. | Receipt/search regression; `configured_finance_and_works_copies_are_used_and_complaints_stay_private` |
| 15 P2 | Residents cannot create business account | Organisation POST creates owner membership and audit; form available without existing memberships. | Receipt/search/organisation regression; browser keyboard/zoom organisation creation |
| 16 P2 | Per-form original missing/inaccessible | Multipart bulk import matches PDFs by `source_file`; persisted FK references protect pending sources from GC/backup loss. Authenticated original-form route and builder link. | `bulk_forms_keep_distinct_originals_and_enforce_source_access` checks two distinct bytes, missing-source validation, GC survival and resident denial |
| 17 P2 | Exhibition approves source, no withdrawal, numeric ID | Saved burned-redaction PNG preview; NSH lookup; manager withdraw clears published blob IDs and public routes stop serving. Poppler tests fail if utilities are absent. | `exhibition_approver_sees_redaction_nsh_lookup_and_manager_withdraws` checks image pixels, manager gate, public 404 and cleared blob |
| 18 P2 | Accountless answer not delivered | Letter notification includes written body in email and SMS. | `accountless_letter_contains_answer_in_email_and_sms` asserts answer content in both channels |
| 19 P2 | Coverage claims stale/false | Rewrote coverage with actual modules/tests; AI default, complete history and local axe suite corrected; external provider requirement honestly partial. | Documentation review; named test filters checked against source |
| 20 P2 | Demo script rejected/overpromised | Finance approves equipment usage; Priya prepares, Helen issues; PDFs authorised rather than cryptographically signed; certificate invoice at Payment; complaint subject and live bond paths documented; added assisted/offline scenes. README personas/stories and cooperation text corrected. | Existing HTTP journeys, browser stories and smoke; documentation review |
| 21 P2 | No price exemption | Manager approves pre-invoice fee waiver with reason/amount. Both pricing paths reduce fee and include explicit explanatory waiver line and approver in invoice/PDF. | `manager_waivers_apply_to_venue_and_definition_invoices_with_reason` checks venue/definition invoices, permission gate and confirmation coverage |
| 22 P2 | Fresh restore-check broken/no cross-server proof | Backup/restore CLI migrates first. Fresh directory check plus copied DB/blob tree boot verifies dashboard and exact document bytes. | `bootstrap_catalogue_non_demo_mocks_and_cross_directory_restore` |
| 23 P3 | Receipt category absent | Proof of payment upload option; uploads remain evidence and do not create payments. | Receipt/search regression; browser receipt evidence test compares payment/summary before/after |
| 24 P3 | Unscanned files labelled clean | Register `not_scanned`; migration relabels old unchecked blobs, preserves rejected; UI says type-checked, not virus-scanned. Submission/source validation accepts type-checked files. | Receipt regression and `storage::tests::put_dedupes_rejects_and_gc` |
| 25 P3 | Missing duplicate metric/reopened ignores range | Closed-duplicate outcome with shared drill-down; reopening predicate uses events in selected period. | `document_class_retention_and_duplicate_reopened_periods`; `every_metric_count_matches_drilldown_and_denials` |

## Additional smallest changes from explanation table

| Requirement | Fix | Regression |
|---|---|---|
| §6.7 Assignment full journey and clock | Collaborator → replace owner → end → escalate. Assignment timestamp explicitly uses `state.now()`. | `assignment_collaboration_replace_end_escalate_uses_injected_time` |
| §6.12 Resident per-line reschedule | Resident may preview old/new quotes and credit delta; staff still owns mutation. | `resident_reschedule_preview_includes_lines_and_credit_delta` |
| §6.13 Estimate label | Money lines distinguish estimated vs actual minutes. | Existing equipment acceptance journey; browser `equipment estimates label minutes as estimated` |
| §6.20 Document-class retention | `document:<category>` rules record dates per document; case disposal waits for longest date; reclose never shortens recorded dates. | `document_class_retention_and_duplicate_reopened_periods`; existing reclose/disposal tests |
| §6.22 Legacy documents | Attach PDF to validated source-system/source-ID row before import; retain original as staff document. | `legacy_documents_complete_export_and_configurable_endpoints` |
| §6.22 Complete export | Include operations, money, tasks/deadlines and immutable document bytes; tests parse ZIP contents rather than checking only magic prefix. | Same export regression checks populated arrays and exact invoice/original bytes |
| §6.22 Endpoint configuration | Save receiver endpoint through admin API/UI, validate URL, preserve outbox behavior. | Same export/integration regression; existing outage/response-loss tests |
| §6.23 Recovery | Reactivate, reset password, reset TOTP; sessions revoked; forced password replacement enforced by Actor and StaffActor. | `account_recovery_forces_totp_and_password_rotation` |
| §8 Keyboard/text zoom | Browser creates organisation using keyboard at 200% text size, checks focus and horizontal overflow; existing phone/axe suite retained. | `e2e/tests/coverage-fixes.spec.ts`; `accessibility.spec.ts` |
| §9 Cooperation | README includes further development, alongside implementation/migration/training, without promising free support. | Documentation review |

## Verification

All final checks passed on 7 October 2026. No commit is made. `deploy/README.md` is the only changed file under `deploy/`.

| Check | Result |
|---|---|
| `cargo test --manifest-path server/Cargo.toml` | 149 passed: 122 unit, 8 acceptance scenarios, 1 seeded journey, 18 coverage regressions. Three existing illustrative doctests remain ignored. |
| `cargo clippy --all-targets --manifest-path server/Cargo.toml -- -D warnings` | Passed, no warnings. |
| Web `npm run build`, `npm run lint`, `npm test` | Passed; 27 tests across 3 files. Build emits the existing nonfatal bundle-size advisory. |
| `cd e2e && npx playwright test` | 26 passed, including accessibility, browser stories and new coverage regressions. |
| `scripts/smoke-integration.sh` | Three consecutive successful runs using separate fresh data directories; each ended `PASS integration smoke`, sequence exited 0. |
| `git diff --check` | Passed. |

Final execution logs: `/tmp/norfolk-rust-verified.log`, `/tmp/norfolk-clippy-verified.log`, `/tmp/norfolk-web-{build,lint,test}-green.log`, `/tmp/norfolk-e2e-complete.log`, and `/tmp/norfolk-smoke-verified-{1,2,3}.log`. These are local verification records, not repository artifacts.

## Honest limits

DemoPay remains an in-app mock, not a verified external provider sandbox. Self-hosted online checkout and mock routes are disabled; real payment/mail adapters remain future work. Integration endpoints are configurable, without claiming verified Council compatibility. Uploads are type-checked without antivirus scanning. Field offline support is an IndexedDB queue, not a service worker. Accessibility checks are local automated checks, without a CI workflow or claim of complete conformance.
