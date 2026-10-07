export interface Busy {
  start_at: string
  end_at: string
  label: string
}
export interface VenueUnit {
  code: string
  name: string
  active: boolean
  capacity: number | null
  prep_minutes: number
  cleanup_minutes: number
  busy: Busy[]
}
export interface Availability {
  units: VenueUnit[]
  conditions: string
}
export interface Slot {
  unit_code: string
  start_at: string
  end_at: string
  attendees: number
}
export interface Resource {
  id: number
  code: string
  name: string
  kind: string
  active: number
  prep_minutes: number
  cleanup_minutes: number
  capacity: number | null
}
export interface Resources {
  resources: Resource[]
  workers: { id: number; name: string }[]
}
export interface Booking {
  id: number
  status: string
  start_at: string
  end_at: string
  attendees: number
  revision: number
  confirmation_version_id: number | null
}
export interface BookingDetail {
  booking: Booking
  unit: { code: string; name: string }
  conditions: string
  history: {
    revision: number
    unit: string
    start_at: string
    end_at: string
    status: string
    reason: string
  }[]
  conflicts?: Busy[]
  settled?: boolean
  can_manage?: boolean
}
export interface QuoteLine {
  item_code: string
  description: string
  amount_cents: number
  kind: string
}
export interface Preview {
  available: boolean
  conflicts: Busy[]
  old_lines: QuoteLine[]
  new_lines: QuoteLine[]
}
export interface Task {
  id: number
  case_id: number
  kind: string
  title: string
  instructions: string
  assigned_to: number | null
  scheduled_start: string | null
  scheduled_end: string | null
  location_text: string | null
  location_lat: number | null
  location_lng: number | null
  status: string
  result_text: string | null
  revision: number
  checklist: { key: string; label: string; done: boolean }[]
  updates: {
    id: number
    kind: string
    body: string
    blob_id: number | null
    created_at: string
    created_offline_at: string | null
  }[]
}
export interface Equipment {
  request: {
    id: number
    description: string
    requested_hours: number
    site_text: string
    preferred_date: string
    scheduled_start: string | null
    scheduled_end: string | null
    assigned_resource_id: number | null
  }
  usage: {
    id: number
    started_at: string
    ended_at: string
    billable_minutes: number
    downtime_minutes: number
    expenses_cents: number
    approved_at: string | null
  }[]
  invoices: {
    id: number
    number: string
    kind: string
    total_cents: number
    basis_note: string
  }[]
  can_schedule: boolean
  can_approve: boolean
  revision: number
}
export interface Road {
  id: number
  category: string
  location: { lat: number; lng: number }
  status_text: string
  reported_on: string
}
export interface CalendarEntry {
  id: number
  resource_id: number
  source: string
  label: string
  case_id: number | null
  start_at: string
  end_at: string
  event_start: string | null
  event_end: string | null
  booking_id: number | null
}
export interface CalendarData {
  resources: Resource[]
  entries: CalendarEntry[]
  requests: {
    id: number
    case_id: number
    unit_code: string
    unit_name: string
    title: string
    start_at: string
    end_at: string
  }[]
}
