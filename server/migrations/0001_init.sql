-- Norfolk ServiceHub — core schema (shared contract for all modules).
-- Conventions:
--   * ids: INTEGER PRIMARY KEY; human-facing references use separate unique columns (cases.number, invoices.number).
--   * timestamps: TEXT, RFC 3339 UTC with milliseconds, e.g. 2026-10-07T03:15:00.000Z. Local dates (holidays, price
--     effective dates) are TEXT 'YYYY-MM-DD' in Pacific/Norfolk.
--   * money: INTEGER cents (AUD). Never REAL.
--   * JSON: TEXT validated by the application.
--   * Append-only history tables are never UPDATEd except where a column is explicitly documented as mutable.
-- Module ownership is noted per section; a module never writes another module's tables directly — it calls the
-- owning module's Rust API (see docs/ARCHITECTURE.md).

PRAGMA foreign_keys = ON;

-- =====================================================================================================
-- PLATFORM (owner: platform)
-- =====================================================================================================

CREATE TABLE users (
    id              INTEGER PRIMARY KEY,
    email           TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name    TEXT NOT NULL,
    phone           TEXT,
    kind            TEXT NOT NULL CHECK (kind IN ('resident', 'staff')),
    password_hash   TEXT,                                   -- argon2id; NULL = cannot log in with password
    totp_secret     TEXT,                                   -- base32; staff only
    totp_enabled    INTEGER NOT NULL DEFAULT 0 CHECK (totp_enabled IN (0, 1)),
    totp_last_step  INTEGER,                                -- replay protection: last accepted TOTP time step
    persona_key     TEXT UNIQUE,                            -- demo persona id (e.g. 'alexey'); NULL for real users
    job_title       TEXT,
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT NOT NULL
);

-- Capabilities. A user may hold several roles. scope_service_id limits a role to one service (NULL = all).
CREATE TABLE role_grants (
    id                INTEGER PRIMARY KEY,
    user_id           INTEGER NOT NULL REFERENCES users(id),
    role              TEXT NOT NULL CHECK (role IN (
                          'intake', 'specialist', 'finance', 'field_worker',
                          'manager', 'sysadmin', 'complaints_officer')),
    scope_service_id  INTEGER REFERENCES services(id),
    granted_by        INTEGER REFERENCES users(id),
    granted_at        TEXT NOT NULL,
    revoked_at        TEXT
);
CREATE INDEX role_grants_user ON role_grants(user_id) WHERE revoked_at IS NULL;

-- Authority to approve/issue decisions. Separate from roles: sysadmin never implies it.
-- Invariant (enforced in code): granted_by holds an active 'manager' role and granted_by <> user_id.
CREATE TABLE decision_authorities (
    id              INTEGER PRIMARY KEY,
    user_id         INTEGER NOT NULL REFERENCES users(id),
    decision_type   TEXT NOT NULL,                          -- e.g. 'development_approval', 'planning_certificate'
    service_id      INTEGER REFERENCES services(id),        -- NULL = any service using decision_type
    granted_by      INTEGER NOT NULL REFERENCES users(id),
    granted_at      TEXT NOT NULL,
    revoked_at      TEXT
);

CREATE TABLE sessions (
    token_hash      TEXT PRIMARY KEY,                       -- sha256(hex) of the opaque cookie token
    user_id         INTEGER NOT NULL REFERENCES users(id),
    csrf_token      TEXT NOT NULL,
    mfa_passed      INTEGER NOT NULL DEFAULT 0 CHECK (mfa_passed IN (0, 1)),
    created_at      TEXT NOT NULL,
    last_seen_at    TEXT NOT NULL,
    expires_at      TEXT NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE login_attempts (
    id              INTEGER PRIMARY KEY,
    key             TEXT NOT NULL,                          -- 'email:<email>' or 'ip:<addr>' or 'totp:<user_id>'
    at              TEXT NOT NULL,
    success         INTEGER NOT NULL CHECK (success IN (0, 1))
);
CREATE INDEX login_attempts_key ON login_attempts(key, at);

CREATE TABLE organisations (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    abn             TEXT,
    created_at      TEXT NOT NULL
);

-- Business accounts: users acting for an organisation. Revoked members immediately lose access to org cases.
CREATE TABLE memberships (
    id                  INTEGER PRIMARY KEY,
    organisation_id     INTEGER NOT NULL REFERENCES organisations(id),
    user_id             INTEGER REFERENCES users(id),        -- NULL until invite accepted
    invite_email        TEXT NOT NULL COLLATE NOCASE,
    invite_token_hash   TEXT UNIQUE,
    role                TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    status              TEXT NOT NULL CHECK (status IN ('invited', 'active', 'revoked')),
    invited_by          INTEGER REFERENCES users(id),
    created_at          TEXT NOT NULL,
    accepted_at         TEXT,
    revoked_at          TEXT,
    revoked_by          INTEGER REFERENCES users(id)
);
CREATE UNIQUE INDEX memberships_active ON memberships(organisation_id, user_id) WHERE status = 'active';

CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY,
    at              TEXT NOT NULL,
    actor_user_id   INTEGER REFERENCES users(id),
    action          TEXT NOT NULL,                          -- dotted verb, e.g. 'case.submit', 'refund.complete'
    entity_type     TEXT NOT NULL,
    entity_id       INTEGER,
    details_json    TEXT NOT NULL DEFAULT '{}',
    ip              TEXT
);
CREATE INDEX audit_log_entity ON audit_log(entity_type, entity_id);

CREATE TABLE settings (
    key             TEXT PRIMARY KEY,                       -- e.g. 'notify.customer_care_email', 'demo.reset_hours'
    value_json      TEXT NOT NULL,
    updated_by      INTEGER REFERENCES users(id),
    updated_at      TEXT NOT NULL
);

-- Immutable stored files. Content-addressed; metadata only. Bytes live in DATA_DIR/blobs/<sha256[0..2]>/<sha256>.
CREATE TABLE blobs (
    id              INTEGER PRIMARY KEY,
    sha256          TEXT NOT NULL UNIQUE,
    size_bytes      INTEGER NOT NULL,
    mime            TEXT NOT NULL,
    original_name   TEXT NOT NULL,
    scan_status     TEXT NOT NULL CHECK (scan_status IN ('clean', 'rejected')),
    created_by      INTEGER REFERENCES users(id),
    created_at      TEXT NOT NULL
);

-- Durable background jobs (also used as the transactional outbox for notifications/integrations).
CREATE TABLE jobs (
    id                  INTEGER PRIMARY KEY,
    kind                TEXT NOT NULL,                      -- e.g. 'notify.deliver', 'integration.deliver', 'deadline.sweep'
    payload_json        TEXT NOT NULL,
    idempotency_key     TEXT UNIQUE,                        -- enqueue is a no-op if key exists
    status              TEXT NOT NULL CHECK (status IN ('pending', 'running', 'done', 'failed', 'dead')),
    attempts            INTEGER NOT NULL DEFAULT 0,
    max_attempts        INTEGER NOT NULL DEFAULT 8,
    run_after           TEXT NOT NULL,
    lease_until         TEXT,
    last_error          TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX jobs_ready ON jobs(status, run_after);

-- Idempotent POST responses (submit, payment confirmation, offline sync…). Same key + different request hash = 409.
CREATE TABLE idempotency_keys (
    actor_user_id   INTEGER NOT NULL,                       -- 0 for unauthenticated (webhooks use their own tables)
    scope           TEXT NOT NULL,                          -- operation name, e.g. 'case.submit'
    key             TEXT NOT NULL,
    request_hash    TEXT NOT NULL,
    status_code     INTEGER NOT NULL,
    response_json   TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    PRIMARY KEY (actor_user_id, scope, key)
);

-- Notifications: in-app (bell) and outbound email/SMS (delivered through the mock gateway by a job).
CREATE TABLE notifications (
    id              INTEGER PRIMARY KEY,
    user_id         INTEGER REFERENCES users(id),           -- NULL for people without an account (phone intake)
    channel         TEXT NOT NULL CHECK (channel IN ('in_app', 'email', 'sms')),
    to_address      TEXT,                                   -- email/phone for outbound channels
    case_id         INTEGER REFERENCES cases(id),
    subject         TEXT NOT NULL,
    body            TEXT NOT NULL,
    link            TEXT,                                   -- SPA path, e.g. '/my/cases/12'
    status          TEXT NOT NULL CHECK (status IN ('queued', 'sent', 'failed', 'read')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    external_id     TEXT,                                   -- id returned by the mail/SMS gateway
    created_at      TEXT NOT NULL,
    sent_at         TEXT,
    read_at         TEXT
);
CREATE INDEX notifications_user ON notifications(user_id, status);

CREATE TABLE holidays (
    id              INTEGER PRIMARY KEY,
    calendar        TEXT NOT NULL DEFAULT 'norfolk',
    date            TEXT NOT NULL,                          -- local date
    name            TEXT NOT NULL,
    source          TEXT NOT NULL,                          -- URL or 'demo'
    UNIQUE (calendar, date)
);

CREATE TABLE backup_runs (
    id              INTEGER PRIMARY KEY,
    kind            TEXT NOT NULL CHECK (kind IN ('backup', 'restore_check')),
    status          TEXT NOT NULL CHECK (status IN ('running', 'ok', 'failed')),
    started_at      TEXT NOT NULL,
    finished_at     TEXT,
    location        TEXT,
    details_json    TEXT NOT NULL DEFAULT '{}'
);

-- =====================================================================================================
-- SERVICES CATALOG (owner: services)
-- =====================================================================================================

CREATE TABLE services (
    id              INTEGER PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    category        TEXT NOT NULL,                          -- 'Venues', 'Planning & Building', 'Works & Roads', 'Feedback', …
    module          TEXT NOT NULL CHECK (module IN (
                        'generic', 'venue_booking', 'equipment_hire', 'building',
                        'planning_certificate', 'road_issue', 'complaint')),
    department      TEXT NOT NULL,                          -- owning team, e.g. 'Customer Care', 'Planning', 'Works Depot'
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT NOT NULL
);

-- Published versions are immutable. Cases reference the exact version they were submitted against.
CREATE TABLE service_versions (
    id                      INTEGER PRIMARY KEY,
    service_id              INTEGER NOT NULL REFERENCES services(id),
    version                 INTEGER NOT NULL,
    status                  TEXT NOT NULL CHECK (status IN ('draft', 'published', 'retired')),
    definition_json         TEXT NOT NULL,                  -- see docs/ARCHITECTURE.md "Service definition"
    source_blob_id          INTEGER REFERENCES blobs(id),   -- original council form (PDF) this was derived from
    source_note             TEXT,
    created_by              INTEGER REFERENCES users(id),
    created_at              TEXT NOT NULL,
    published_by            INTEGER REFERENCES users(id),
    published_at            TEXT,
    UNIQUE (service_id, version)
);
CREATE UNIQUE INDEX service_versions_one_published ON service_versions(service_id) WHERE status = 'published';

CREATE TABLE service_imports (
    id              INTEGER PRIMARY KEY,
    uploaded_by     INTEGER NOT NULL REFERENCES users(id),
    filename        TEXT NOT NULL,
    blob_id         INTEGER NOT NULL REFERENCES blobs(id),
    status          TEXT NOT NULL CHECK (status IN ('validated', 'has_errors', 'applied')),
    report_json     TEXT NOT NULL,                          -- per-definition validation errors / created draft ids
    created_at      TEXT NOT NULL
);

CREATE VIRTUAL TABLE service_search USING fts5(
    service_id UNINDEXED, name, summary, keywords, tokenize = 'porter unicode61'
);

-- =====================================================================================================
-- CASES (owner: cases core = platform; workflow/submission = services)
-- =====================================================================================================

CREATE TABLE building_projects (
    id              INTEGER PRIMARY KEY,
    reference       TEXT NOT NULL UNIQUE,                   -- e.g. 'BP-2026-0004'
    title           TEXT NOT NULL,
    property_ref    TEXT NOT NULL,                          -- e.g. 'Portion 44h, Taylors Road'
    owner_user_id   INTEGER REFERENCES users(id),
    owner_org_id    INTEGER REFERENCES organisations(id),
    created_at      TEXT NOT NULL
);

CREATE TABLE cases (
    id                      INTEGER PRIMARY KEY,
    number                  TEXT UNIQUE,                    -- assigned at submission, e.g. 'NSH-2026-000123'; NULL while draft
    service_id              INTEGER NOT NULL REFERENCES services(id),
    service_version_id      INTEGER NOT NULL REFERENCES service_versions(id),
    module                  TEXT NOT NULL,                  -- copy of services.module at creation
    title                   TEXT NOT NULL,
    status                  TEXT NOT NULL CHECK (status IN (
                                'draft', 'submitted', 'in_progress', 'waiting_on_applicant',
                                'completed', 'refused', 'withdrawn', 'cancelled', 'closed_duplicate')),
    current_step            TEXT,                           -- workflow step key from the frozen definition
    applicant_user_id       INTEGER REFERENCES users(id),   -- NULL for assisted intake without an account
    applicant_org_id        INTEGER REFERENCES organisations(id),
    applicant_name          TEXT NOT NULL,
    applicant_email         TEXT,
    applicant_phone         TEXT,
    intake_channel          TEXT NOT NULL CHECK (intake_channel IN (
                                'online', 'phone', 'walk_in', 'email', 'post', 'legacy_import')),
    recorded_by_user_id     INTEGER REFERENCES users(id),   -- staff who keyed in an assisted request
    confidential            INTEGER NOT NULL DEFAULT 0 CHECK (confidential IN (0, 1)),
    public_map              INTEGER NOT NULL DEFAULT 0 CHECK (public_map IN (0, 1)),   -- road issues only
    property_ref            TEXT,
    location_text           TEXT,
    location_lat            REAL,
    location_lng            REAL,
    building_project_id     INTEGER REFERENCES building_projects(id),
    reopened_count          INTEGER NOT NULL DEFAULT 0,
    legal_hold              INTEGER NOT NULL DEFAULT 0 CHECK (legal_hold IN (0, 1)),
    retention_until         TEXT,                           -- local date after which disposal may be proposed
    revision                INTEGER NOT NULL DEFAULT 1,     -- optimistic concurrency for staff commands
    created_at              TEXT NOT NULL,
    submitted_at            TEXT,
    closed_at               TEXT,
    updated_at              TEXT NOT NULL
);
CREATE INDEX cases_status ON cases(status);
CREATE INDEX cases_applicant ON cases(applicant_user_id);
CREATE INDEX cases_org ON cases(applicant_org_id);

CREATE TABLE case_drafts (
    case_id         INTEGER PRIMARY KEY REFERENCES cases(id),
    answers_json    TEXT NOT NULL DEFAULT '{}',
    updated_at      TEXT NOT NULL
);

-- Frozen at submission: answers + full definition snapshot. Never updated.
CREATE TABLE submissions (
    id                          INTEGER PRIMARY KEY,
    case_id                     INTEGER NOT NULL UNIQUE REFERENCES cases(id),
    service_version_id          INTEGER NOT NULL REFERENCES service_versions(id),
    definition_snapshot_json    TEXT NOT NULL,
    definition_sha256           TEXT NOT NULL,
    answers_json                TEXT NOT NULL,
    submitted_by                INTEGER REFERENCES users(id),
    submitted_at                TEXT NOT NULL
);

-- Representatives acting for the applicant (e.g. a builder). Revocation is checked on every request.
CREATE TABLE case_representatives (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    user_id             INTEGER NOT NULL REFERENCES users(id),
    basis               TEXT NOT NULL,                      -- 'Owner authorisation letter dated …'
    evidence_document_id INTEGER REFERENCES documents(id),
    status              TEXT NOT NULL CHECK (status IN ('active', 'revoked')),
    created_at          TEXT NOT NULL,
    revoked_at          TEXT,
    revoked_by          INTEGER REFERENCES users(id)
);

CREATE TABLE case_assignments (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    user_id         INTEGER NOT NULL REFERENCES users(id),
    role            TEXT NOT NULL CHECK (role IN ('owner', 'collaborator')),
    assigned_by     INTEGER REFERENCES users(id),
    reason          TEXT,
    assigned_at     TEXT NOT NULL,
    ended_at        TEXT,
    ended_reason    TEXT
);
CREATE UNIQUE INDEX case_assignments_one_owner ON case_assignments(case_id) WHERE role = 'owner' AND ended_at IS NULL;

-- Explicit exclusion overrides any role (e.g. the staff member a complaint is about).
CREATE TABLE case_access_denials (
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    user_id         INTEGER NOT NULL REFERENCES users(id),
    reason          TEXT NOT NULL,
    created_by      INTEGER REFERENCES users(id),
    created_at      TEXT NOT NULL,
    PRIMARY KEY (case_id, user_id)
);

-- Timeline. visibility: 'applicant' = shown to applicant and staff; 'staff' = staff with case access only.
CREATE TABLE case_events (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    at              TEXT NOT NULL,
    actor_user_id   INTEGER REFERENCES users(id),
    kind            TEXT NOT NULL,                          -- 'submitted', 'assigned', 'step_changed', 'message', 'payment_confirmed', …
    visibility      TEXT NOT NULL CHECK (visibility IN ('applicant', 'staff')),
    summary         TEXT NOT NULL,                          -- plain-English sentence
    data_json       TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX case_events_case ON case_events(case_id, at);

-- Conversation with the applicant. Internal notes live in a different table on purpose.
CREATE TABLE case_messages (
    id                      INTEGER PRIMARY KEY,
    case_id                 INTEGER NOT NULL REFERENCES cases(id),
    author_user_id          INTEGER REFERENCES users(id),
    from_staff              INTEGER NOT NULL CHECK (from_staff IN (0, 1)),
    body                    TEXT NOT NULL,
    document_version_id     INTEGER REFERENCES document_versions(id),  -- message about a specific document version
    requires_response       INTEGER NOT NULL DEFAULT 0 CHECK (requires_response IN (0, 1)),
    resolved_at             TEXT,                                       -- when applicant answered a requires_response
    created_at              TEXT NOT NULL
);
CREATE INDEX case_messages_case ON case_messages(case_id, created_at);

CREATE TABLE internal_notes (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    author_user_id  INTEGER NOT NULL REFERENCES users(id),
    body            TEXT NOT NULL,
    created_at      TEXT NOT NULL
);

CREATE TABLE case_links (
    id              INTEGER PRIMARY KEY,
    from_case_id    INTEGER NOT NULL REFERENCES cases(id),
    to_case_id      INTEGER NOT NULL REFERENCES cases(id),
    kind            TEXT NOT NULL CHECK (kind IN (
                        'modification_of', 'review_of', 'duplicate_of', 'follow_up_of', 'related')),
    note            TEXT,
    created_by      INTEGER REFERENCES users(id),
    created_at      TEXT NOT NULL,
    UNIQUE (from_case_id, to_case_id, kind),
    CHECK (from_case_id <> to_case_id)
);

-- Complaints: the staff member(s) the complaint is about. Rows here also produce case_access_denials.
CREATE TABLE complaint_subjects (
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    staff_user_id   INTEGER NOT NULL REFERENCES users(id),
    PRIMARY KEY (case_id, staff_user_id)
);

CREATE VIRTUAL TABLE case_search USING fts5(
    case_id UNINDEXED, number, title, applicant_name, property_ref, body, tokenize = 'porter unicode61'
);

-- =====================================================================================================
-- DEADLINES (owner: services/workflow)
-- =====================================================================================================

CREATE TABLE deadlines (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    kind                TEXT NOT NULL,                      -- 'completeness', 'decision', 'response', …
    label               TEXT NOT NULL,
    basis               TEXT NOT NULL CHECK (basis IN ('business', 'calendar')),
    duration_days       INTEGER NOT NULL,
    pausable            INTEGER NOT NULL CHECK (pausable IN (0, 1)),
    max_pause_days      INTEGER,                            -- cumulative cap; NULL = not pausable
    started_at          TEXT NOT NULL,
    due_at              TEXT NOT NULL,                      -- recomputed (with history in case_events) on pause/resume
    status              TEXT NOT NULL CHECK (status IN ('running', 'paused', 'met', 'breached', 'cancelled')),
    met_at              TEXT,
    breached_at         TEXT,
    policy_json         TEXT NOT NULL                       -- the policy object from the frozen definition
);
CREATE INDEX deadlines_open ON deadlines(status, due_at);

CREATE TABLE deadline_pauses (
    id              INTEGER PRIMARY KEY,
    deadline_id     INTEGER NOT NULL REFERENCES deadlines(id),
    reason          TEXT NOT NULL,
    message_id      INTEGER REFERENCES case_messages(id),
    started_at      TEXT NOT NULL,
    ended_at        TEXT,
    ended_reason    TEXT CHECK (ended_reason IN ('applicant_responded', 'cap_reached', 'staff_resumed'))
);

-- =====================================================================================================
-- DOCUMENTS, DECISIONS, BUILDING, EXHIBITIONS (owner: documents)
-- =====================================================================================================

CREATE TABLE documents (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    requirement_key     TEXT,                               -- documents[].key from the definition, if any
    category            TEXT NOT NULL,                      -- 'application', 'plans', 'evidence', 'decision', 'certificate', 'receipt', 'photo', …
    title               TEXT NOT NULL,
    visibility          TEXT NOT NULL CHECK (visibility IN ('applicant', 'staff')),
    created_by          INTEGER REFERENCES users(id),
    created_at          TEXT NOT NULL,
    disposed_at         TEXT                                -- set by retention disposal; versions' blobs removed
);
CREATE INDEX documents_case ON documents(case_id);

CREATE TABLE document_versions (
    id                  INTEGER PRIMARY KEY,
    document_id         INTEGER NOT NULL REFERENCES documents(id),
    version             INTEGER NOT NULL,
    blob_id             INTEGER NOT NULL REFERENCES blobs(id),
    uploaded_by         INTEGER REFERENCES users(id),
    note                TEXT,                               -- 'Revised per comment #12'
    uploaded_at         TEXT NOT NULL,
    UNIQUE (document_id, version)
);

CREATE TABLE document_comments (
    id                      INTEGER PRIMARY KEY,
    document_version_id     INTEGER NOT NULL REFERENCES document_versions(id),
    author_user_id          INTEGER NOT NULL REFERENCES users(id),
    visibility              TEXT NOT NULL CHECK (visibility IN ('applicant', 'internal')),
    body                    TEXT NOT NULL,
    resolved_at             TEXT,
    resolved_by_version_id  INTEGER REFERENCES document_versions(id),
    created_at              TEXT NOT NULL
);

CREATE TABLE decision_templates (
    id              INTEGER PRIMARY KEY,
    code            TEXT NOT NULL,
    version         INTEGER NOT NULL,
    name            TEXT NOT NULL,
    decision_type   TEXT NOT NULL,
    body_template   TEXT NOT NULL,                          -- text with {{placeholders}} from an allow-list
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at      TEXT NOT NULL,
    UNIQUE (code, version)
);

-- One row per separate approval (development approval and building approval are different rows).
CREATE TABLE decisions (
    id                          INTEGER PRIMARY KEY,
    case_id                     INTEGER NOT NULL REFERENCES cases(id),
    decision_type               TEXT NOT NULL,
    outcome                     TEXT NOT NULL CHECK (outcome IN ('approved', 'approved_with_conditions', 'refused')),
    reasons                     TEXT NOT NULL,
    conditions                  TEXT,
    status                      TEXT NOT NULL CHECK (status IN ('draft', 'pending_approval', 'issued', 'returned')),
    template_id                 INTEGER REFERENCES decision_templates(id),
    prepared_by                 INTEGER NOT NULL REFERENCES users(id),
    approved_by                 INTEGER REFERENCES users(id),  -- must hold decision_authorities for decision_type
    returned_reason             TEXT,
    output_document_id          INTEGER REFERENCES documents(id),
    supersedes_decision_id      INTEGER REFERENCES decisions(id),  -- modification approval referencing the original
    created_at                  TEXT NOT NULL,
    issued_at                   TEXT
);

-- Exactly which document versions a decision was based on.
CREATE TABLE decision_evidence (
    decision_id             INTEGER NOT NULL REFERENCES decisions(id),
    document_version_id     INTEGER NOT NULL REFERENCES document_versions(id),
    PRIMARY KEY (decision_id, document_version_id)
);

CREATE TABLE exhibitions (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    title           TEXT NOT NULL,
    summary         TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('draft', 'open', 'closed', 'withdrawn')),
    opens_at        TEXT,
    closes_at       TEXT,
    prepared_by     INTEGER NOT NULL REFERENCES users(id),
    approved_by     INTEGER REFERENCES users(id),
    created_at      TEXT NOT NULL
);

-- Published copy is a NEW blob produced by rasterise-and-burn redaction; the source version is never served publicly.
CREATE TABLE exhibition_items (
    id                          INTEGER PRIMARY KEY,
    exhibition_id               INTEGER NOT NULL REFERENCES exhibitions(id),
    source_document_version_id  INTEGER NOT NULL REFERENCES document_versions(id),
    title                       TEXT NOT NULL,
    redactions_json             TEXT NOT NULL DEFAULT '[]',  -- [{page, x, y, w, h}] in page fractions 0..1
    published_blob_id           INTEGER REFERENCES blobs(id),
    created_at                  TEXT NOT NULL
);

CREATE TABLE public_submissions (
    id              INTEGER PRIMARY KEY,
    exhibition_id   INTEGER NOT NULL REFERENCES exhibitions(id),
    name            TEXT NOT NULL,
    email           TEXT NOT NULL,
    body            TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('received', 'considered')),
    created_at      TEXT NOT NULL
);

-- =====================================================================================================
-- RESOURCES, BOOKINGS, EQUIPMENT, FIELD TASKS (owner: operations)
-- =====================================================================================================

-- Atomic resources: individual rooms and individual equipment items.
CREATE TABLE resources (
    id              INTEGER PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('space', 'equipment')),
    venue           TEXT,                                   -- 'Rawson Hall' for spaces
    description     TEXT NOT NULL DEFAULT '',
    capacity        INTEGER,
    prep_minutes    INTEGER NOT NULL DEFAULT 0,             -- buffer before
    cleanup_minutes INTEGER NOT NULL DEFAULT 0,             -- buffer after
    price_item_code TEXT,                                   -- hourly rate item for equipment
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1))
);

-- What a person can book: 'Rawson Hall — Main Hall', '— Supper Room', '— Whole venue' (= both rooms).
CREATE TABLE bookable_units (
    id              INTEGER PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    venue           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    fee_item_code   TEXT,                                   -- price item for hire
    deposit_item_code TEXT,                                 -- price item for bond
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1))
);
CREATE TABLE bookable_unit_resources (
    unit_id         INTEGER NOT NULL REFERENCES bookable_units(id),
    resource_id     INTEGER NOT NULL REFERENCES resources(id),
    PRIMARY KEY (unit_id, resource_id)
);

CREATE TABLE bookings (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    unit_id         INTEGER NOT NULL REFERENCES bookable_units(id),
    status          TEXT NOT NULL CHECK (status IN ('requested', 'confirmed', 'cancelled', 'completed')),
    start_at        TEXT NOT NULL,                          -- event start (without buffers)
    end_at          TEXT NOT NULL,
    attendees       INTEGER,
    revision        INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    CHECK (end_at > start_at)
);

-- Immutable history of every booking change (request, confirmation, reschedule, cancellation).
CREATE TABLE booking_revisions (
    id              INTEGER PRIMARY KEY,
    booking_id      INTEGER NOT NULL REFERENCES bookings(id),
    revision        INTEGER NOT NULL,
    unit_id         INTEGER NOT NULL REFERENCES bookable_units(id),
    start_at        TEXT NOT NULL,
    end_at          TEXT NOT NULL,
    status          TEXT NOT NULL,
    changed_by      INTEGER REFERENCES users(id),
    reason          TEXT,
    created_at      TEXT NOT NULL,
    UNIQUE (booking_id, revision)
);

-- Blocking occupancy of atomic resources, including buffers: [start_at, end_at) half-open, UTC.
-- Only confirmed bookings, equipment assignments and maintenance blocks occupy.
CREATE TABLE occupancies (
    id              INTEGER PRIMARY KEY,
    resource_id     INTEGER NOT NULL REFERENCES resources(id),
    source          TEXT NOT NULL CHECK (source IN ('booking', 'equipment', 'maintenance')),
    booking_id      INTEGER REFERENCES bookings(id),
    case_id         INTEGER REFERENCES cases(id),
    label           TEXT NOT NULL,
    start_at        TEXT NOT NULL,
    end_at          TEXT NOT NULL,
    active          INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at      TEXT NOT NULL,
    CHECK (end_at > start_at)
);
CREATE INDEX occupancies_resource ON occupancies(resource_id, active, start_at);

-- Defence in depth: the booking service checks availability inside BEGIN IMMEDIATE; this trigger rejects any
-- overlapping active occupancy that slips through another write path.
CREATE TRIGGER occupancies_no_overlap_insert
BEFORE INSERT ON occupancies
WHEN NEW.active = 1
BEGIN
    SELECT RAISE(ABORT, 'occupancy_conflict')
    WHERE EXISTS (
        SELECT 1 FROM occupancies o
        WHERE o.resource_id = NEW.resource_id AND o.active = 1
          AND o.start_at < NEW.end_at AND o.end_at > NEW.start_at
    );
END;

CREATE TRIGGER occupancies_no_overlap_update
BEFORE UPDATE OF active, start_at, end_at, resource_id ON occupancies
WHEN NEW.active = 1
BEGIN
    SELECT RAISE(ABORT, 'occupancy_conflict')
    WHERE EXISTS (
        SELECT 1 FROM occupancies o
        WHERE o.id <> NEW.id AND o.resource_id = NEW.resource_id AND o.active = 1
          AND o.start_at < NEW.end_at AND o.end_at > NEW.start_at
    );
END;

CREATE TABLE tasks (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    kind                TEXT NOT NULL CHECK (kind IN (
                            'venue_prep', 'venue_inspection', 'equipment_job', 'road_inspection',
                            'road_repair', 'site_inspection', 'general')),
    title               TEXT NOT NULL,
    instructions        TEXT NOT NULL,                      -- only what the worker needs; no applicant documents
    assigned_to         INTEGER REFERENCES users(id),
    scheduled_start     TEXT,
    scheduled_end       TEXT,
    location_text       TEXT,
    location_lat        REAL,
    location_lng        REAL,
    checklist_json      TEXT NOT NULL DEFAULT '[]',         -- [{key, label, done}]
    status              TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done', 'cancelled')),
    result_text         TEXT,
    revision            INTEGER NOT NULL DEFAULT 1,
    created_by          INTEGER REFERENCES users(id),
    created_at          TEXT NOT NULL,
    completed_at        TEXT
);
CREATE INDEX tasks_assignee ON tasks(assigned_to, status);

-- Field updates; client_command_id makes offline re-sends idempotent.
CREATE TABLE task_updates (
    id                  INTEGER PRIMARY KEY,
    task_id             INTEGER NOT NULL REFERENCES tasks(id),
    client_command_id   TEXT NOT NULL UNIQUE,
    author_user_id      INTEGER NOT NULL REFERENCES users(id),
    kind                TEXT NOT NULL CHECK (kind IN ('note', 'photo', 'checklist', 'status', 'result')),
    body                TEXT,
    blob_id             INTEGER REFERENCES blobs(id),
    created_offline_at  TEXT,                               -- device timestamp when the worker saved it
    created_at          TEXT NOT NULL
);

-- Equipment hire: estimate (requested) vs actual usage recorded by the operator.
CREATE TABLE equipment_requests (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    description         TEXT NOT NULL,                      -- what the applicant needs and for what
    requested_hours     INTEGER,                            -- applicant's estimate
    preferred_date      TEXT,
    site_text           TEXT,
    assigned_resource_id INTEGER REFERENCES resources(id),
    task_id             INTEGER REFERENCES tasks(id),
    created_at          TEXT NOT NULL
);

CREATE TABLE equipment_usage (
    id                  INTEGER PRIMARY KEY,
    equipment_request_id INTEGER NOT NULL REFERENCES equipment_requests(id),
    resource_id         INTEGER NOT NULL REFERENCES resources(id),
    operator_user_id    INTEGER NOT NULL REFERENCES users(id),
    started_at          TEXT NOT NULL,
    ended_at            TEXT NOT NULL,
    downtime_minutes    INTEGER NOT NULL DEFAULT 0,
    billable_minutes    INTEGER NOT NULL,                   -- (ended - started) - downtime, computed in code
    expenses_cents      INTEGER NOT NULL DEFAULT 0,
    expenses_note       TEXT,
    recorded_by         INTEGER NOT NULL REFERENCES users(id),
    recorded_at         TEXT NOT NULL,
    approved_by         INTEGER REFERENCES users(id),
    approved_at         TEXT,
    CHECK (ended_at > started_at)
);

-- =====================================================================================================
-- FINANCE (owner: finance)
-- =====================================================================================================

CREATE TABLE price_items (
    id              INTEGER PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE,                   -- 'HALL_MAIN_DAY', 'HALL_BOND', 'PLANNING_CERT', 'EXCAVATOR_HOUR'
    name            TEXT NOT NULL,
    unit            TEXT NOT NULL CHECK (unit IN ('each', 'hour', 'session', 'day')),
    kind            TEXT NOT NULL CHECK (kind IN ('fee', 'deposit')),
    gst_applicable  INTEGER NOT NULL DEFAULT 0 CHECK (gst_applicable IN (0, 1))
);

-- Effective-dated prices; intervals for one item never overlap (checked in code + trigger below).
CREATE TABLE price_versions (
    id              INTEGER PRIMARY KEY,
    price_item_id   INTEGER NOT NULL REFERENCES price_items(id),
    amount_cents    INTEGER NOT NULL CHECK (amount_cents >= 0),
    effective_from  TEXT NOT NULL,                          -- local date inclusive
    effective_to    TEXT,                                   -- local date exclusive; NULL = open-ended
    created_by      INTEGER REFERENCES users(id),
    created_at      TEXT NOT NULL,
    CHECK (effective_to IS NULL OR effective_to > effective_from)
);

CREATE TRIGGER price_versions_no_overlap
BEFORE INSERT ON price_versions
BEGIN
    SELECT RAISE(ABORT, 'price_overlap')
    WHERE EXISTS (
        SELECT 1 FROM price_versions p
        WHERE p.price_item_id = NEW.price_item_id
          AND (p.effective_to IS NULL OR p.effective_to > NEW.effective_from)
          AND (NEW.effective_to IS NULL OR NEW.effective_to > p.effective_from)
    );
END;

-- Estimates never post to the ledger. Issued invoices are immutable; corrections use credit notes.
CREATE TABLE invoices (
    id                      INTEGER PRIMARY KEY,
    case_id                 INTEGER NOT NULL REFERENCES cases(id),
    number                  TEXT NOT NULL UNIQUE,           -- 'INV-2026-00042', 'EST-…', 'CN-…'
    kind                    TEXT NOT NULL CHECK (kind IN ('estimate', 'invoice', 'credit_note')),
    status                  TEXT NOT NULL CHECK (status IN ('draft', 'issued', 'void')),
    pricing_date            TEXT NOT NULL,                  -- local date used to look up price_versions
    total_cents             INTEGER NOT NULL,
    basis_note              TEXT,                           -- human explanation ('Actual usage 5h 30m per job card')
    credits_invoice_id      INTEGER REFERENCES invoices(id),
    created_by              INTEGER REFERENCES users(id),
    created_at              TEXT NOT NULL,
    issued_at               TEXT
);
CREATE INDEX invoices_case ON invoices(case_id);

CREATE TABLE invoice_lines (
    id                  INTEGER PRIMARY KEY,
    invoice_id          INTEGER NOT NULL REFERENCES invoices(id),
    price_item_id       INTEGER REFERENCES price_items(id),
    price_version_id    INTEGER REFERENCES price_versions(id),  -- the exact rate used (copied below)
    kind                TEXT NOT NULL CHECK (kind IN ('fee', 'deposit')),
    description         TEXT NOT NULL,
    quantity_milli      INTEGER NOT NULL,                   -- 1000 = 1 unit; 5.5 hours = 5500
    unit_amount_cents   INTEGER NOT NULL,
    amount_cents        INTEGER NOT NULL,                   -- round_half_up(quantity_milli * unit / 1000)
    calc_json           TEXT NOT NULL DEFAULT '{}'          -- inputs used, e.g. {"billable_minutes":330,"source":"equipment_usage#3"}
);

-- Confirmed money only. A receipt photo is a document, never a payment.
CREATE TABLE payments (
    id                  INTEGER PRIMARY KEY,
    source              TEXT NOT NULL CHECK (source IN ('provider', 'bank_transfer', 'counter')),
    external_id         TEXT NOT NULL,                      -- provider payment id / bank transaction id / counter receipt no
    amount_cents        INTEGER NOT NULL CHECK (amount_cents > 0),
    received_at         TEXT NOT NULL,
    payer_name          TEXT,
    reference           TEXT,
    case_id             INTEGER REFERENCES cases(id),       -- NULL while unmatched
    status              TEXT NOT NULL CHECK (status IN ('confirmed', 'reversed')),
    recorded_by         INTEGER REFERENCES users(id),
    created_at          TEXT NOT NULL,
    UNIQUE (source, external_id)
);

CREATE TABLE payment_allocations (
    id                  INTEGER PRIMARY KEY,
    payment_id          INTEGER NOT NULL REFERENCES payments(id),
    invoice_line_id     INTEGER NOT NULL REFERENCES invoice_lines(id),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents > 0),
    created_by          INTEGER REFERENCES users(id),
    created_at          TEXT NOT NULL,
    reversed_at         TEXT,
    reversed_by         INTEGER REFERENCES users(id),
    reversal_reason     TEXT
);

-- Mock payment provider integration (hosted checkout + signed webhooks).
CREATE TABLE checkout_sessions (
    id                      INTEGER PRIMARY KEY,
    provider_session_id     TEXT NOT NULL UNIQUE,
    case_id                 INTEGER NOT NULL REFERENCES cases(id),
    invoice_id              INTEGER NOT NULL REFERENCES invoices(id),
    amount_cents            INTEGER NOT NULL,
    status                  TEXT NOT NULL CHECK (status IN ('open', 'paid', 'expired', 'failed')),
    created_by              INTEGER REFERENCES users(id),
    created_at              TEXT NOT NULL,
    completed_at            TEXT
);

CREATE TABLE provider_events (
    id              INTEGER PRIMARY KEY,
    event_id        TEXT NOT NULL UNIQUE,                   -- dedupe on provider event id
    type            TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    signature_valid INTEGER NOT NULL CHECK (signature_valid IN (0, 1)),
    received_at     TEXT NOT NULL,
    processed_at    TEXT,
    result          TEXT
);

CREATE TABLE statement_imports (
    id              INTEGER PRIMARY KEY,
    filename        TEXT NOT NULL,
    blob_id         INTEGER NOT NULL REFERENCES blobs(id),
    file_sha256     TEXT NOT NULL UNIQUE,                   -- same file twice is rejected
    imported_by     INTEGER NOT NULL REFERENCES users(id),
    imported_at     TEXT NOT NULL,
    row_count       INTEGER NOT NULL
);

CREATE TABLE statement_rows (
    id              INTEGER PRIMARY KEY,
    import_id       INTEGER NOT NULL REFERENCES statement_imports(id),
    bank_txn_id     TEXT NOT NULL,
    txn_date        TEXT NOT NULL,
    amount_cents    INTEGER NOT NULL,
    payer_name      TEXT,
    description     TEXT NOT NULL,
    reference       TEXT,
    status          TEXT NOT NULL CHECK (status IN ('unmatched', 'matched', 'ignored', 'duplicate')),
    suggested_case_id INTEGER REFERENCES cases(id),
    payment_id      INTEGER REFERENCES payments(id),
    resolved_by     INTEGER REFERENCES users(id),
    resolved_at     TEXT,
    note            TEXT
);
CREATE UNIQUE INDEX statement_rows_txn ON statement_rows(bank_txn_id) WHERE status <> 'duplicate';

-- Deposit lifecycle. Held money is a liability, never fee revenue.
CREATE TABLE deposit_decisions (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    invoice_line_id     INTEGER NOT NULL REFERENCES invoice_lines(id),  -- the deposit line
    refund_cents        INTEGER NOT NULL CHECK (refund_cents >= 0),
    retain_cents        INTEGER NOT NULL CHECK (retain_cents >= 0),
    reason              TEXT NOT NULL,                      -- shown to the applicant
    calc_json           TEXT NOT NULL DEFAULT '{}',         -- itemised retention, e.g. [{"label":"Extra cleaning","cents":8000}]
    decided_by          INTEGER NOT NULL REFERENCES users(id),
    decided_at          TEXT NOT NULL
);

-- A refund is 'completed' only after provider confirmation (webhook) or finance confirming the bank transfer.
CREATE TABLE refunds (
    id                  INTEGER PRIMARY KEY,
    case_id             INTEGER NOT NULL REFERENCES cases(id),
    payment_id          INTEGER NOT NULL REFERENCES payments(id),
    deposit_decision_id INTEGER REFERENCES deposit_decisions(id),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents > 0),
    method              TEXT NOT NULL CHECK (method IN ('provider', 'bank_transfer')),
    status              TEXT NOT NULL CHECK (status IN ('requested', 'processing', 'completed', 'failed')),
    provider_refund_id  TEXT UNIQUE,
    reason              TEXT NOT NULL,
    requested_by        INTEGER NOT NULL REFERENCES users(id),
    created_at          TEXT NOT NULL,
    completed_at        TEXT,
    failure_reason      TEXT
);

-- Minimal double-entry ledger. Every money event posts one balanced entry; (source_type, source_id, purpose) unique.
CREATE TABLE journal_entries (
    id              INTEGER PRIMARY KEY,
    at              TEXT NOT NULL,
    case_id         INTEGER REFERENCES cases(id),
    source_type     TEXT NOT NULL,                          -- 'invoice', 'payment', 'allocation', 'refund', 'deposit_decision', 'credit_note'
    source_id       INTEGER NOT NULL,
    purpose         TEXT NOT NULL,
    memo            TEXT NOT NULL,
    UNIQUE (source_type, source_id, purpose)
);
CREATE TABLE journal_lines (
    id              INTEGER PRIMARY KEY,
    entry_id        INTEGER NOT NULL REFERENCES journal_entries(id),
    account         TEXT NOT NULL CHECK (account IN (
                        'bank', 'provider_clearing', 'receivables', 'fee_revenue',
                        'deposit_liability', 'customer_credit', 'unallocated_receipts', 'refunds_payable')),
    debit_cents     INTEGER NOT NULL DEFAULT 0 CHECK (debit_cents >= 0),
    credit_cents    INTEGER NOT NULL DEFAULT 0 CHECK (credit_cents >= 0),
    CHECK ((debit_cents = 0) <> (credit_cents = 0))
);

-- =====================================================================================================
-- INTEGRATIONS, IMPORT, RETENTION (owner: integrations / records)
-- =====================================================================================================

CREATE TABLE external_systems (
    code            TEXT PRIMARY KEY,                       -- 'content_manager', 'civica_altitude'
    name            TEXT NOT NULL,
    base_url        TEXT NOT NULL,                          -- points at the in-binary mock in the demo
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

-- Sender side. operation_id is stable across retries; the receiver dedupes on it.
CREATE TABLE integration_deliveries (
    id              INTEGER PRIMARY KEY,
    system_code     TEXT NOT NULL REFERENCES external_systems(code),
    case_id         INTEGER REFERENCES cases(id),
    operation_id    TEXT NOT NULL UNIQUE,
    kind            TEXT NOT NULL,                          -- 'record.case_closed', 'document.decision', 'payment.receipt'
    payload_json    TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'accepted', 'failed', 'dead')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    external_ref    TEXT,                                   -- receiver's record number once accepted
    last_error      TEXT,
    next_attempt_at TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

-- Receiver side of the MOCK external systems (separate state from the sender, as a real remote system would be).
CREATE TABLE mock_external_records (
    id              INTEGER PRIMARY KEY,
    system_code     TEXT NOT NULL,
    operation_id    TEXT NOT NULL,
    external_ref    TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    received_at     TEXT NOT NULL,
    UNIQUE (system_code, operation_id)
);
CREATE TABLE mock_system_state (
    system_code     TEXT PRIMARY KEY,
    outage          INTEGER NOT NULL DEFAULT 0 CHECK (outage IN (0, 1)),       -- admin toggles to simulate downtime
    drop_responses  INTEGER NOT NULL DEFAULT 0 CHECK (drop_responses IN (0, 1)) -- accept but time out ("response lost")
);

CREATE TABLE legacy_import_batches (
    id              INTEGER PRIMARY KEY,
    filename        TEXT NOT NULL,
    blob_id         INTEGER NOT NULL REFERENCES blobs(id),
    status          TEXT NOT NULL CHECK (status IN ('previewed', 'imported')),
    report_json     TEXT NOT NULL,
    imported_by     INTEGER NOT NULL REFERENCES users(id),
    created_at      TEXT NOT NULL
);
CREATE TABLE legacy_import_records (
    id                      INTEGER PRIMARY KEY,
    batch_id                INTEGER NOT NULL REFERENCES legacy_import_batches(id),
    source_system           TEXT NOT NULL,
    source_id               TEXT NOT NULL,
    status                  TEXT NOT NULL CHECK (status IN ('imported', 'duplicate', 'error')),
    case_id                 INTEGER REFERENCES cases(id),
    duplicate_of_case_id    INTEGER REFERENCES cases(id),
    message                 TEXT
);
CREATE UNIQUE INDEX legacy_import_records_source ON legacy_import_records(source_system, source_id) WHERE status = 'imported';

CREATE TABLE retention_rules (
    id              INTEGER PRIMARY KEY,
    record_class    TEXT NOT NULL UNIQUE,                   -- e.g. 'venue_booking', 'building', 'complaint', 'finance'
    retain_years    INTEGER NOT NULL,
    trigger_event   TEXT NOT NULL CHECK (trigger_event IN ('case_closed', 'document_created')),
    description     TEXT NOT NULL
);

CREATE TABLE legal_holds (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    reason          TEXT NOT NULL,
    placed_by       INTEGER NOT NULL REFERENCES users(id),
    placed_at       TEXT NOT NULL,
    released_by     INTEGER REFERENCES users(id),
    released_at     TEXT
);

CREATE TABLE disposal_events (
    id              INTEGER PRIMARY KEY,
    case_id         INTEGER NOT NULL REFERENCES cases(id),
    document_id     INTEGER REFERENCES documents(id),
    actor_user_id   INTEGER NOT NULL REFERENCES users(id),
    reason          TEXT NOT NULL,
    at              TEXT NOT NULL
);
