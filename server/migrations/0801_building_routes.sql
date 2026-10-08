-- Audit-2 slice A: building fees (N-01), approval scope and multi-original modifications (N-02),
-- explicit public exhibition states (N-03). Rows of the demo fee scale are seeded by finance (prices.rs),
-- because demo resets wipe every table.

-- Stored fee scale (data, not code). One row per band: applies when over_cents < cost <= up_to_cents
-- (the first band also covers a zero cost); fee = base + rate per $1,000 over over_cents, pro rata to the cent.
CREATE TABLE fee_scale_bands (
 id INTEGER PRIMARY KEY,
 scale_code TEXT NOT NULL,
 effective_from TEXT NOT NULL,
 effective_to TEXT,
 over_cents INTEGER NOT NULL CHECK(over_cents>=0),
 up_to_cents INTEGER,
 base_cents INTEGER NOT NULL CHECK(base_cents>=0),
 rate_cents_per_1000 INTEGER NOT NULL CHECK(rate_cents_per_1000>=0),
 source_note TEXT NOT NULL,
 UNIQUE(scale_code,effective_from,over_cents)
);

-- Explainable fee assessments, append-only per case (latest version is current).
CREATE TABLE building_fee_assessments (
 id INTEGER PRIMARY KEY,
 case_id INTEGER NOT NULL REFERENCES cases(id),
 version INTEGER NOT NULL,
 method TEXT NOT NULL CHECK(method IN ('schedule','manual')),
 rule TEXT NOT NULL,
 inputs_json TEXT NOT NULL,
 lines_json TEXT NOT NULL,
 explanation TEXT NOT NULL,
 amount_cents INTEGER NOT NULL CHECK(amount_cents>0),
 reason TEXT,
 assessed_by INTEGER NOT NULL REFERENCES users(id),
 assessed_at TEXT NOT NULL,
 -- Gross fee covered by issued invoices once this assessment took effect (NULL until invoiced).
 charged_cents INTEGER,
 -- Supplementary invoice or credit note issued for a re-assessment after invoicing.
 adjustment_invoice_id INTEGER REFERENCES invoices(id),
 UNIQUE(case_id,version)
);

-- Staff-confirmed approval scope: {"approvals":[types]} for a primary application, {"originals":[decision ids]}
-- for a modification. Append-only; the latest row is current; source='staff' means confirmed.
CREATE TABLE building_approval_scopes (
 id INTEGER PRIMARY KEY,
 case_id INTEGER NOT NULL REFERENCES cases(id),
 scope_json TEXT NOT NULL,
 source TEXT NOT NULL CHECK(source IN ('applicant','staff')),
 reason TEXT NOT NULL,
 set_by INTEGER REFERENCES users(id),
 set_at TEXT NOT NULL
);

-- A modification may name several original approvals (DA and/or BA).
CREATE TABLE building_original_approvals_v2 (
 case_id INTEGER NOT NULL REFERENCES cases(id),
 decision_id INTEGER NOT NULL REFERENCES decisions(id),
 PRIMARY KEY(case_id,decision_id)
);
INSERT INTO building_original_approvals_v2(case_id,decision_id) SELECT case_id,decision_id FROM building_original_approvals;
DROP TABLE building_original_approvals;
ALTER TABLE building_original_approvals_v2 RENAME TO building_original_approvals;

-- Exhibition: formal early termination (status stays 'closed'; the API reports 'terminated') and a recorded
-- consideration of every public submission.
ALTER TABLE exhibitions ADD COLUMN terminated_at TEXT;
ALTER TABLE exhibitions ADD COLUMN terminated_by INTEGER REFERENCES users(id);
ALTER TABLE exhibitions ADD COLUMN termination_reason TEXT;
ALTER TABLE exhibitions ADD COLUMN consideration_summary TEXT;
ALTER TABLE exhibitions ADD COLUMN considered_by INTEGER REFERENCES users(id);
ALTER TABLE exhibitions ADD COLUMN considered_at TEXT;
ALTER TABLE public_submissions ADD COLUMN outcome TEXT;
ALTER TABLE public_submissions ADD COLUMN considered_by INTEGER REFERENCES users(id);
ALTER TABLE public_submissions ADD COLUMN considered_at TEXT;
UPDATE public_submissions SET outcome='Marked considered before consideration outcomes were recorded.' WHERE status='considered';

-- "Exhibition not required for this case", with its mandatory reason.
CREATE TABLE exhibition_not_required (
 id INTEGER PRIMARY KEY,
 case_id INTEGER NOT NULL REFERENCES cases(id),
 reason TEXT NOT NULL,
 decided_by INTEGER REFERENCES users(id),
 decided_at TEXT NOT NULL
);
