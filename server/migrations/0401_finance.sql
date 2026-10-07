-- Finance-only projections and provider state. The provider does not read our checkout/payment tables.
CREATE TABLE finance_payment_balances (
 payment_id INTEGER PRIMARY KEY REFERENCES payments(id),
 credit_cents INTEGER NOT NULL DEFAULT 0 CHECK(credit_cents >= 0),
 suspense_cents INTEGER NOT NULL DEFAULT 0 CHECK(suspense_cents >= 0)
);
CREATE UNIQUE INDEX finance_one_deposit_decision ON deposit_decisions(invoice_line_id);
ALTER TABLE refunds ADD COLUMN bank_reference TEXT;
CREATE TABLE finance_invoice_documents (
 invoice_id INTEGER PRIMARY KEY REFERENCES invoices(id),
 document_id INTEGER NOT NULL REFERENCES documents(id),
 document_version_id INTEGER NOT NULL REFERENCES document_versions(id)
);
CREATE VIEW finance_line_balances AS
SELECT l.*,
 COALESCE((SELECT SUM(a.amount_cents) FROM payment_allocations a JOIN payments p ON p.id=a.payment_id
  WHERE a.invoice_line_id=l.id AND a.reversed_at IS NULL AND p.status='confirmed'),0) AS paid_cents,
 COALESCE((SELECT SUM(cl.amount_cents) FROM invoice_lines cl JOIN invoices ci ON ci.id=cl.invoice_id
  WHERE ci.kind='credit_note' AND ci.status='issued' AND ci.credits_invoice_id=l.invoice_id
  AND CAST(json_extract(cl.calc_json,'$.original_line_id') AS INTEGER)=l.id),0) AS credited_cents
FROM invoice_lines l;
CREATE TABLE mock_pay_sessions (
 session_id TEXT PRIMARY KEY, amount_cents INTEGER NOT NULL CHECK(amount_cents>0),
 currency TEXT NOT NULL CHECK(currency='AUD'), reference TEXT NOT NULL, return_url TEXT NOT NULL,
 metadata_json TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('open','paid','failed')),
 payment_id TEXT UNIQUE, created_at TEXT NOT NULL
);
CREATE TABLE mock_pay_refunds (
 refund_id TEXT PRIMARY KEY, payment_id TEXT NOT NULL, amount_cents INTEGER NOT NULL CHECK(amount_cents>0),
 idempotency_key TEXT NOT NULL UNIQUE, status TEXT NOT NULL CHECK(status IN ('pending','succeeded','failed')),
 created_at TEXT NOT NULL
);
CREATE TABLE mock_pay_webhook_attempts (
 id INTEGER PRIMARY KEY, event_id TEXT NOT NULL UNIQUE, payload_json TEXT NOT NULL,
 attempts INTEGER NOT NULL DEFAULT 0, delivered_at TEXT, last_error TEXT, created_at TEXT NOT NULL
);
