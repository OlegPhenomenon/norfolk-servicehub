# Application to Modify Development and/or Building Approval — form → site mapping

Source: NIRC form "Application for Modification of Development Approval and/or Building Approval", updated 12 March 2024
(`https://www.nirc.gov.au/files/assets/public/v/1/planning-development/documents/application_to_modify_development_and_or_building_approval_form_18_03_24.pdf`, 9 pages; field list in `docs/research/forms.md` §3).
Site service: `modify-approval` (`server/src/services/seed.rs::modify_approval_form`). Keys below are the answer keys in
the frozen submission snapshot. "Required" is enforced by the server at submission (`services::validation::validate_answers`
for fields, document requirements for attachments). The paper form marks mandatory fields with `*`, but those marks are
not preserved in the published text; requiredness therefore follows what each section *asks for* ("must", "you need to
submit", "please attach"), and optional status is used where the form says "if", "may" or "if in doubt".

## Applicant sections (pages 3–7)

| Form section / item | Site field key or named attachment | Required? | Basis |
|---|---|---|---|
| Header: Application No. | Case number `NSH-YYYY-NNNNNN` (assigned on submission) | automatic | Office reference; the site numbers every case. |
| Header: Original application number | `original_approval` (decision picker; slice A owns the multi-approval version) | yes | The modification must identify the approval it modifies (Planning Act 2002 Div 4 / Building Act 2002 Div 2). |
| 1. Applicant's details — Name Applicant 1, Applicant 2 (first/last) | `applicants[]` rows: `first_name`, `last_name` (one row per applicant, add rows for more than two) | yes, at least one row | Section 1 identifies the applicant (may be an agent acting for the landowner). |
| 1. Postal address | `applicants[].postal_address` | yes | Council's written notices are sent to the applicant. |
| 1. Phone No., Mob. No. | `applicants[].phone`, `applicants[].mobile` | no | Contact numbers; one of phone/email is normally given, but the form does not make either mandatory. |
| 1. Email(s) | `applicants[].email` | no | As above. Account email of the submitting user is also on the case. |
| 1. Signature Applicant 1 / 2 | `declaration` checkbox ("I/we, the applicant(s), declare that the information in this application is correct") | yes | Digital lodgement replaces the wet signature with the signed-in applicant's declaration. |
| 2. Landowner's details (if not the applicant) | `landowners_are_applicants` (yes/no); when "no": `landowners[]` rows `first_name`, `last_name`, `postal_address`, `phone`, `mobile`, `email` | question yes; rows yes when "no" | Section 2 applies "if not the Applicant". |
| 2. Signature(s) of all landowners (consent to lodge the modification only) | Attachment **`owners_consent` "Signed consent of all landowners"** + per-row `landowners[].consent` checkbox | yes | The form requires every landowner's signature. Signatures cannot be captured by a checkbox, so the signed page is a required attachment; its help text says so and staff review it in the case **Documents** tab ("Required and optional documents" checklist shows it as Required/Attached). |
| 3. Property description — Address | `property_ref` | yes | Identifies the land; also becomes the case/project property reference. |
| 3. Portion No., Lot No., Section No., Land Area (two rows on paper) | `parcels[]` rows: `portion` (required), `lot`, `section`, `land_area` | yes, at least one row | Section 3. Add rows for every parcel. Lot/section/area are optional per row because not every portion has them. |
| 3. Land tenure | `land_tenure` (Freehold / Crown Lease / Vacant Crown Land / Road Reserve / Un-alienated Crown Land) | yes | Single tick list on the form. |
| 3. Zoning | `zoning` (12 zones as printed) | yes | Single tick list on the form. |
| 3. "Please attach a copy of the Title Search" | Attachment **`title_search` "Copy of title search"** | yes | "Please attach" — the form asks for it with every application. |
| 3. What is the land currently used for? | `current_use` | yes | Direct question. |
| 4. Type(s) of use, development and/or building (tick all relevant) | `use_types` multiselect (13 options as printed); `use_types_other` when "Other" | yes ("other" text required when ticked) | "Please tick all relevant boxes"; "Other (please specify)". |
| 4. Notes (Clause 101 NI Plan 2002; Schedule 1 Building Regulations 2002) | Informational; shown in the service description/research, not an answer | — | Guidance only. |
| 5. Type of modification (tick boxes) | `modification_types` multiselect: `minor_error`, `conditions`, `lapse_date`, `other` | yes | "Please indicate the type of modification … by ticking the appropriate box(es)" — several may apply. |
| 5. Minor error — describe the modification and its expected impact | `minor_error_description` (shown and required when `minor_error` is ticked) | when ticked | Each ticked type has its own description line. |
| 5. Condition(s) — describe the modification and its expected impact | `conditions_description` (when `conditions`) | when ticked | As above. |
| 5. Lapse date — proposed date and reasons | `proposed_lapse_date` (date) and `lapse_date_reasons` (when `lapse_date`) | when ticked | "Describe the proposed date for the extension … and the reasons". |
| 5. Any other modification — describe and impact | `other_modification_description` (when `other`) | when ticked | As above. |
| 5. "You need to submit … a full description of the expected impacts …, including relevant plans, drawings and compliance with relevant controls" | Attachment **`modification_plans` "Description of expected impacts, with relevant plans and drawings"** | yes | "You need to submit with your application". |
| 6. Substantially the same — the proposed modified use or development including all modifications since the original approval | `modified_proposal` | yes | Division 4 Planning Act / Division 2 Building Act only apply when the result is substantially the same; Council needs this comparison. |
| 6. Changes in the external environment since the original approval | `external_environment_changes` | yes | Same test (section 6 second bullet). |
| 7. Application fees — total estimated cost of building and works $ | `estimated_cost` (AUD, number) | yes | "It is necessary to specify the total estimated cost … to determine the fees"; required "prior to acceptance". Fee calculation itself: slice A (fee assessment). |
| 8. Other approvals (tick relevant legislation) | `other_approvals` multiselect (11 Acts + Other); `other_approvals_details` when "Other" | no ("other" text required when ticked) | "May need approvals …"; "If in doubt, please contact the Planning Office". |
| 9. Supporting information — "Please list what you have attached" | Attachment `supporting` "Other supporting information (plans, drawings, photographs)"; the case Documents list *is* the list of attachments | no | "You can support your application with additional material". |
| Lodgement details / "What now" (10 working days) | Service description and the completeness deadline (`deadlines[completeness]`) | — | Informational. |

## Office sections (pages 7–9)

| Form section / item | How staff record it on the site | Owner |
|---|---|---|
| Official use only — Receiving Officer, Date | Case timeline: submission time and the *Check request* (`intake`) step run with the officer who completed it | existing workflow |
| Consideration of adequacy — Application satisfactory to lodge and accept Yes/No | *Check request* step: **Complete this step** (accept) or **Request information** (not yet satisfactory) / **Refuse request**; reason is recorded | existing workflow |
| Additional information required before acceptance | **Request information** message to the applicant (case pauses on `waiting_on_applicant`); document comments with "request new version" | existing workflow |
| Planning Act — accepted as a Modification of DA Yes/No; Building Act — accepted as a Modification of BA Yes/No | Approval scope recorded by staff for the case (DA, BA or both) | slice A (N-02 scope) |
| Application acceptance — Officer, Date | Step completion event of *Check request* (actor + time) in the timeline | existing workflow |
| Internal use — Fee Paid $, Receipt No., Receipt Date | Money tab: invoice, payment and receipt (DemoPay in the demo) | slice A (N-01 fee assessment) + finance |
| Internal use — Modification Application No. | Case number | automatic |
| Internal use — Combined / DA only / BA only | Approval scope (as above) | slice A |
| Internal use — Original DA and/or BA number | `original_approval` link (case links "modification of" and the building project history) | slice A |
| Internal use — Modification Application Fee advice attached | Fee assessment and issued invoice PDF in the Money tab | slice A |

## Not carried over

Nothing the applicant sections ask for is dropped. Wet signatures are replaced by the declaration (applicants) and the
signed-consent attachment (landowners), as described above. Page-layout items (BLOCK LETTERS, "attach a separate sheet")
do not apply to a web form: group rows replace extra sheets.
