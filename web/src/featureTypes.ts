/**
 * Extension points that feature slices fill in (see `registry.ts`). Import with
 *   import type { NavItem, CasePanel, FieldComponent } from '@/featureTypes'
 */
import type { FC } from 'react'
import type { CaseSummary, FieldDef, Role, ServiceDefinition } from './api/types'
import type { IconName } from './ui/Icon'

/** A sidebar/menu entry. `to` is an absolute path (`/staff/bookings`). */
export interface NavItem {
  to: string
  label: string
  icon?: IconName
  /** Visible only to holders of at least one of these roles. Omit = every staff member (or every resident in residentNav). */
  roles?: Role[]
  /** Match `to` exactly when highlighting the active item (for index pages). */
  end?: boolean
}

/** A tab on the case page (the case page itself belongs to the services slice). */
export interface CasePanel {
  /** Unique, used in `?tab=` — prefix with your module: `operations.booking`. */
  key: string
  label: string
  /** Who sees the tab: staff workspace, applicant (`/my`) view, or both. */
  audience: 'staff' | 'applicant' | 'both'
  /** Show this panel for this case? Usually checks `c.module`; `definition` is the case's frozen service definition. */
  applies: (c: CaseSummary, definition: ServiceDefinition) => boolean
  Component: FC<{ caseId: number }>
}

/** Props every form widget receives from the service form renderer. */
export interface FieldComponentProps {
  field: FieldDef
  /** Current answer; `undefined` until set. Must be the canonical value for `field.type` (see api/types `AnswerValue`). */
  value: unknown
  onChange: (value: unknown) => void
  /** Server/client validation message for this field. */
  error?: string
  disabled?: boolean
}

/** Custom form widget for one `FieldDef.type` (e.g. `booking_slot`). */
export type FieldComponent = FC<FieldComponentProps>
