/**
 * Shared platform types. These mirror the backend JSON exactly (snake_case keys).
 * Timestamps are RFC 3339 UTC strings (`2026-10-07T03:15:00.000Z`); local dates are `YYYY-MM-DD`
 * (Pacific/Norfolk). Money is integer cents (AUD) — format with `<Money>` / `formatMoney`.
 *
 * Feature slices: put your own response types in `features/<module>/types.ts`, not here.
 */

/** RFC 3339 UTC timestamp string. */
export type Timestamp = string
/** Local calendar date `YYYY-MM-DD` (Pacific/Norfolk). */
export type LocalDate = string
/** Integer cents, AUD. */
export type Cents = number

// ---------------------------------------------------------------------------
// Identity & session
// ---------------------------------------------------------------------------

/** Staff capability roles (`role_grants.role`). Residents have no roles. */
export type Role =
  | 'intake'
  | 'specialist'
  | 'finance'
  | 'field_worker'
  | 'manager'
  | 'sysadmin'
  | 'complaints_officer'

export const ROLE_LABELS: Record<Role, string> = {
  intake: 'Customer Care (intake)',
  specialist: 'Specialist',
  finance: 'Finance',
  field_worker: 'Field worker',
  manager: 'Manager',
  sysadmin: 'Systems administrator',
  complaints_officer: 'Complaints officer',
}

export type UserKind = 'resident' | 'staff'

export interface User {
  id: number
  name: string
  email: string
  kind: UserKind
  /** Demo persona id (`alexey`, `olga`, …); null for real users. */
  persona_key: string | null
  job_title: string | null
  /** Staff only: has TOTP been enrolled? Staff without it must enrol at `/staff/settings/2fa`. */
  totp_enabled: boolean
}

/** `GET /api/me` */
export interface Me {
  user: User | null
  /** Active role names held by the user (deduplicated). Empty for residents/anonymous. */
  roles: Role[]
  /** Send as `X-CSRF-Token` on every non-GET request (the api client does this for you). */
  csrf_token: string
  /** Staff session that has not passed TOTP yet. */
  mfa_required: boolean
  demo_mode: boolean
  /** Demo mode: when the data will next be reset. */
  next_reset_at: Timestamp | null
  /** Show the (mock) AI helper. Optional for forward compatibility. */
  ai_enabled?: boolean
}

/** `POST /api/auth/totp/enroll` */
export interface TotpEnrolment {
  secret: string
  otpauth_url: string
  /** Raw `<svg>` markup of the QR code. */
  qr_svg: string
}

// ---------------------------------------------------------------------------
// Demo mode
// ---------------------------------------------------------------------------

/** `GET /api/demo/personas` */
export interface Persona {
  persona: string
  name: string
  kind: UserKind
  job_title: string | null
  roles: Role[]
  organisation?: string | null
}

/** `GET /api/demo/authenticator` — live TOTP codes of staff personas. */
export interface AuthenticatorCode {
  persona: string
  name: string
  code: string
  seconds_left: number
}

/** `GET /api/demo/mailbox` — outbound email/SMS that the demo gateway would have delivered. */
export interface MailboxMessage {
  id: number
  channel?: 'email' | 'sms'
  to: string | null
  subject: string
  body: string
  status: string
  error: string | null
  created_at: Timestamp
  sent_at?: Timestamp | null
  /** Id returned by the (mock) gateway. */
  external_id?: string | null
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

export interface Notification {
  id: number
  subject: string
  body: string
  /** SPA path to open, e.g. `/my/cases/12`. */
  link: string | null
  case_id: number | null
  created_at: Timestamp
  read_at: Timestamp | null
}

/** `GET /api/notifications` */
export interface NotificationList {
  items: Notification[]
  unread_count: number
}

// ---------------------------------------------------------------------------
// Cases (rows of `cases`; full case views are owned by the services slice)
// ---------------------------------------------------------------------------

export type CaseStatus =
  | 'draft'
  | 'submitted'
  | 'in_progress'
  | 'waiting_on_applicant'
  | 'completed'
  | 'refused'
  | 'withdrawn'
  | 'cancelled'
  | 'closed_duplicate'

/** `services.module` — selects module hooks and panels. Frozen on the case at creation. */
export type ServiceModule =
  | 'generic'
  | 'venue_booking'
  | 'equipment_hire'
  | 'building'
  | 'planning_certificate'
  | 'road_issue'
  | 'complaint'

export type IntakeChannel = 'online' | 'phone' | 'walk_in' | 'email' | 'post' | 'legacy_import'

/**
 * Minimal case shape passed to `CasePanel.applies` and shown in lists.
 * The case page (services slice) loads it and decides which module panels to show.
 */
export interface CaseSummary {
  id: number
  /** `NSH-2026-000123`; null while draft. */
  number: string | null
  service_id: number
  service_name: string
  module: ServiceModule
  title: string
  status: CaseStatus
  /** Workflow step key from the frozen definition. */
  current_step: string | null
  applicant_name: string
  confidential: boolean
  revision: number
  created_at: Timestamp
  submitted_at: Timestamp | null
  updated_at: Timestamp
}

// ---------------------------------------------------------------------------
// Service definition (`service_versions.definition_json`, ARCHITECTURE §5)
// ---------------------------------------------------------------------------

export type FieldType =
  | 'text'
  | 'textarea'
  | 'number'
  | 'date'
  | 'time'
  | 'email'
  | 'phone'
  | 'select'
  | 'multiselect'
  | 'checkbox'
  | 'property_ref'
  | 'location'
  | 'booking_slot'
  | 'equipment_request'
  | 'decision_ref'

export interface FieldOption {
  value: string
  label: string
}

/** Conditional display: show the field only while `answers[field] === equals`. */
export interface ShowIf {
  field: string
  equals: string | number | boolean
}

export interface FieldDef {
  key: string
  type: FieldType
  label: string
  /** Applies only while the field is shown (see `show_if`). */
  required?: boolean
  /** Help text shown under the label. */
  hint?: string
  max_length?: number
  min?: number
  max?: number
  /** `select` / `multiselect`. */
  options?: FieldOption[]
  show_if?: ShowIf
  /** Module-specific configuration, e.g. `venue: "Rawson Hall"` for `booking_slot`. */
  [extra: string]: unknown
}

// Canonical answer values — identical in renderer, validator and hooks.

/** `booking_slot` */
export interface BookingSlotValue {
  unit_code: string
  start_at: Timestamp
  end_at: Timestamp
  attendees: number
}

/** `equipment_request` */
export interface EquipmentRequestValue {
  description: string
  requested_hours: number
  preferred_date: LocalDate
  site_text: string
}

/** `location` */
export interface LocationValue {
  lat: number
  lng: number
  description: string
}

/** `decision_ref` */
export interface DecisionRefValue {
  decision_id: number
}

/**
 * Answer value by field type:
 * text/textarea/email/phone/property_ref/select/time(`HH:MM`)/date(`YYYY-MM-DD`) → string;
 * number → number; checkbox → boolean; multiselect → string[]; others → objects above.
 */
export type AnswerValue =
  | string
  | number
  | boolean
  | string[]
  | BookingSlotValue
  | EquipmentRequestValue
  | LocationValue
  | DecisionRefValue

/** Answers keyed by `FieldDef.key`. Hidden fields are dropped before submit. */
export type Answers = Record<string, AnswerValue>

export interface DocumentRequirement {
  key: string
  label: string
  required: boolean
  /** MIME types, e.g. `application/pdf`. */
  accept: string[]
  public_candidate: boolean
}

export type StepKind = 'review' | 'payment' | 'decision' | 'task' | 'module' | 'complete'

export interface StepDef {
  key: string
  kind: StepKind
  /** Staff role that works this step (absent for `complete`). */
  role?: Role
  label: string
  /** What the applicant sees while the case is at this step. */
  applicant_label: string
  /** `module` steps: e.g. `operations.booking_confirmed`. */
  handler?: string
  optional?: boolean
  /** `decision` steps: every type must be issued to leave the step. */
  decision_types?: string[]
  /** `task` steps: e.g. `venue_prep`. */
  task_kind?: string
}

export interface DeadlineDef {
  kind: string
  label: string
  days: number
  basis: 'business' | 'calendar'
  /** `submitted` or `step:<key>`. */
  starts: string
  stops: string
  pausable: boolean
  max_pause_days?: number
}

export interface PricingItem {
  item: string
  quantity: number
}

/** `service_versions.definition_json` */
export interface ServiceDefinition {
  conditions?: string[]
  summary: string
  outcome: string
  who_can_apply: string
  price_note: string
  keywords: string[]
  fields: FieldDef[]
  documents: DocumentRequirement[]
  workflow: { steps: StepDef[] }
  deadlines: DeadlineDef[]
  pricing: PricingItem[]
}
