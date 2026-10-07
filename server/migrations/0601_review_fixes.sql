-- Provenance belongs to the document, not a caller-controlled category.
ALTER TABLE documents ADD COLUMN generated INTEGER NOT NULL DEFAULT 0 CHECK(generated IN (0,1));
UPDATE documents SET generated=1 WHERE category IN ('decision','letter','certificate','invoice','credit_note','booking_confirmation')
 OR id IN (SELECT output_document_id FROM decisions WHERE output_document_id IS NOT NULL)
 OR id IN (SELECT document_id FROM issued_letters)
 OR id IN (SELECT document_id FROM finance_invoice_documents);
CREATE TABLE case_closure_events (
 id INTEGER PRIMARY KEY,
 case_id INTEGER NOT NULL REFERENCES cases(id),
 generation INTEGER NOT NULL,
 closed_at TEXT NOT NULL,
 UNIQUE(case_id,generation)
);
CREATE TABLE booking_confirmations (
 case_id INTEGER NOT NULL REFERENCES cases(id),
 booking_id INTEGER NOT NULL REFERENCES bookings(id),
 booking_revision INTEGER NOT NULL,
 document_version_id INTEGER NOT NULL UNIQUE REFERENCES document_versions(id),
 PRIMARY KEY(booking_id,booking_revision)
);
INSERT INTO booking_confirmations SELECT case_id,id,revision,confirmation_version_id FROM bookings WHERE confirmation_version_id IS NOT NULL;
CREATE TABLE booking_cancellations (
 booking_id INTEGER PRIMARY KEY REFERENCES bookings(id),
 cancelled_at TEXT NOT NULL,
 unused INTEGER NOT NULL CHECK(unused IN (0,1)),
 actor_user_id INTEGER REFERENCES users(id),
 reason TEXT NOT NULL
);
ALTER TABLE equipment_requests ADD COLUMN status TEXT NOT NULL DEFAULT 'requested';
-- Existing cancellation revisions preserve when hire was cancelled and who did it.
INSERT INTO booking_cancellations(booking_id,cancelled_at,unused,actor_user_id,reason)
SELECT b.id,r.created_at,COALESCE(julianday(r.created_at)<julianday(b.start_at),0),r.changed_by,
       COALESCE(r.reason,'Recorded booking cancellation')
FROM bookings b JOIN booking_revisions r ON r.booking_id=b.id AND r.revision=b.revision
WHERE b.status='cancelled' AND r.status='cancelled';
