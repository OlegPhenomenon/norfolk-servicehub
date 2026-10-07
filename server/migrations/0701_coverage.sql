-- Do not claim an antivirus scan: preserve explicit rejections, relabel the previous unchecked files.
ALTER TABLE blobs ADD COLUMN previous_scan_status TEXT;
UPDATE blobs SET previous_scan_status=scan_status;
ALTER TABLE blobs DROP COLUMN scan_status;
ALTER TABLE blobs ADD COLUMN scan_status TEXT NOT NULL DEFAULT 'not_scanned' CHECK(scan_status IN ('not_scanned','rejected','clean'));
UPDATE blobs SET scan_status='rejected' WHERE previous_scan_status='rejected';
ALTER TABLE blobs DROP COLUMN previous_scan_status;
CREATE TABLE service_synonyms(token TEXT PRIMARY KEY, replacement TEXT NOT NULL);
INSERT INTO service_synonyms VALUES ('party','hall'),('birthday','hall'),('wedding','hall'),('venue','hall'),('digger','equipment'),('excavator','equipment'),('pothole','road'),('hole','road'),('da','development'),('permit','development');
CREATE TABLE case_price_waivers (
 id INTEGER PRIMARY KEY,
 case_id INTEGER NOT NULL REFERENCES cases(id),
 item_code TEXT NOT NULL REFERENCES price_items(code),
 amount_cents INTEGER NOT NULL CHECK(amount_cents>0),
 reason TEXT NOT NULL,
 approved_by INTEGER NOT NULL REFERENCES users(id),
 approved_at TEXT NOT NULL,
 UNIQUE(case_id,item_code)
);
ALTER TABLE documents ADD COLUMN retention_until TEXT;
CREATE TABLE payment_match_history(id INTEGER PRIMARY KEY, payment_id INTEGER NOT NULL REFERENCES payments(id), case_id INTEGER REFERENCES cases(id), kind TEXT NOT NULL, reason TEXT NOT NULL, changed_by INTEGER REFERENCES users(id), changed_at TEXT NOT NULL);
ALTER TABLE users ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0 CHECK(must_change_password IN (0,1));
CREATE TABLE legacy_import_documents (batch_id INTEGER NOT NULL REFERENCES legacy_import_batches(id), source_system TEXT NOT NULL, source_id TEXT NOT NULL, blob_id INTEGER NOT NULL REFERENCES blobs(id), title TEXT NOT NULL);

CREATE TABLE service_import_sources (import_id INTEGER NOT NULL REFERENCES service_imports(id), filename TEXT NOT NULL, blob_id INTEGER NOT NULL REFERENCES blobs(id), PRIMARY KEY(import_id,filename));
