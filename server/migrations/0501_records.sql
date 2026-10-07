-- Records indexes and safeguards; original shared schema remains unchanged.
CREATE INDEX integration_deliveries_case ON integration_deliveries(case_id,status);
CREATE INDEX integration_deliveries_status ON integration_deliveries(status,system_code);
CREATE INDEX records_cases_retention ON cases(retention_until) WHERE closed_at IS NOT NULL;
CREATE UNIQUE INDEX records_one_active_hold ON legal_holds(case_id) WHERE released_at IS NULL;
CREATE INDEX records_legacy_batch ON legacy_import_records(batch_id);
CREATE INDEX records_mock_operations ON mock_external_records(system_code,operation_id);
CREATE TRIGGER records_retention_nonnegative_insert BEFORE INSERT ON retention_rules
WHEN NEW.retain_years < 0 OR NEW.retain_years > 100
BEGIN SELECT RAISE(ABORT,'invalid retention years'); END;
CREATE TRIGGER records_retention_nonnegative_update BEFORE UPDATE ON retention_rules
WHEN NEW.retain_years < 0 OR NEW.retain_years > 100
BEGIN SELECT RAISE(ABORT,'invalid retention years'); END;
