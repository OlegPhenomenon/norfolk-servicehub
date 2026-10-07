-- Operations-only schedule and provenance fields.
CREATE UNIQUE INDEX bookings_one_per_case ON bookings(case_id);
CREATE UNIQUE INDEX tasks_one_per_step_run ON tasks(step_run_id) WHERE step_run_id IS NOT NULL;
ALTER TABLE bookings ADD COLUMN confirmation_version_id INTEGER REFERENCES document_versions(id);
ALTER TABLE equipment_requests ADD COLUMN operator_user_id INTEGER REFERENCES users(id);
ALTER TABLE equipment_requests ADD COLUMN scheduled_start TEXT;
ALTER TABLE equipment_requests ADD COLUMN scheduled_end TEXT;
CREATE UNIQUE INDEX equipment_requests_case ON equipment_requests(case_id);
ALTER TABLE equipment_usage ADD COLUMN client_command_id TEXT;
CREATE UNIQUE INDEX equipment_usage_command ON equipment_usage(client_command_id) WHERE client_command_id IS NOT NULL;
ALTER TABLE equipment_usage ADD COLUMN final_invoice_id INTEGER REFERENCES invoices(id);
