/**
 * Feature registry (platform-owned). Concatenates the exports of every `features/<module>/` folder so the
 * shell can mount routes, build menus and render case panels / form widgets without knowing the features.
 * Feature slices never edit this file: they fill in their own `routes.tsx`, `nav.ts`, `casePanels.tsx`, `fieldTypes.tsx`.
 */
import type { RouteObject } from 'react-router'
import type { Role } from './api/types'
import type { CasePanel, FieldComponent, NavItem } from './featureTypes'

import * as casesRoutes from './features/cases/routes'
import * as casesNav from './features/cases/nav'
import { casePanels as casesPanels } from './features/cases/casePanels'
import { fieldTypes as casesFields } from './features/cases/fieldTypes'

import * as servicesRoutes from './features/services/routes'
import * as servicesNav from './features/services/nav'
import { casePanels as servicesPanels } from './features/services/casePanels'
import { fieldTypes as servicesFields } from './features/services/fieldTypes'

import * as documentsRoutes from './features/documents/routes'
import * as documentsNav from './features/documents/nav'
import { casePanels as documentsPanels } from './features/documents/casePanels'
import { fieldTypes as documentsFields } from './features/documents/fieldTypes'

import * as operationsRoutes from './features/operations/routes'
import * as operationsNav from './features/operations/nav'
import { casePanels as operationsPanels } from './features/operations/casePanels'
import { fieldTypes as operationsFields } from './features/operations/fieldTypes'

import * as financeRoutes from './features/finance/routes'
import * as financeNav from './features/finance/nav'
import { casePanels as financePanels } from './features/finance/casePanels'
import { fieldTypes as financeFields } from './features/finance/fieldTypes'

import * as recordsRoutes from './features/records/routes'
import * as recordsNav from './features/records/nav'
import { casePanels as recordsPanels } from './features/records/casePanels'
import { fieldTypes as recordsFields } from './features/records/fieldTypes'

const ROUTES = [casesRoutes, servicesRoutes, documentsRoutes, operationsRoutes, financeRoutes, recordsRoutes]
const NAVS = [casesNav, servicesNav, documentsNav, operationsNav, financeNav, recordsNav]

/** Children of PublicLayout (`/`). */
export const publicRoutes: RouteObject[] = ROUTES.flatMap((m) => m.publicRoutes)
/** Children of ResidentLayout (`/my`). */
export const residentRoutes: RouteObject[] = ROUTES.flatMap((m) => m.residentRoutes)
/** Children of StaffLayout (`/staff`). */
export const staffRoutes: RouteObject[] = ROUTES.flatMap((m) => m.staffRoutes)
/** Children of AdminLayout (`/admin`). */
export const adminRoutes: RouteObject[] = ROUTES.flatMap((m) => m.adminRoutes)

export const staffNav: NavItem[] = NAVS.flatMap((m) => m.staffNav)
export const adminNav: NavItem[] = NAVS.flatMap((m) => m.adminNav)
export const residentNav: NavItem[] = NAVS.flatMap((m) => m.residentNav)

/** All case-page tabs, in feature order. Filter with `panelsFor(caseSummary, audience)`. */
export const casePanels: CasePanel[] = [...casesPanels, ...servicesPanels, ...documentsPanels, ...operationsPanels, ...financePanels, ...recordsPanels]

/** Form widgets by `FieldDef.type`. A later feature never silently overrides an earlier one (first wins, dev warning). */
export const fieldTypes: Record<string, FieldComponent> = {}
for (const map of [casesFields, servicesFields, documentsFields, operationsFields, financeFields, recordsFields]) {
  for (const [type, component] of Object.entries(map)) {
    if (fieldTypes[type]) {
      if (import.meta.env.DEV) console.warn(`registry: field type "${type}" is registered twice; keeping the first.`)
      continue
    }
    fieldTypes[type] = component
  }
}

/** Nav items visible to someone holding `roles` (items without `roles` are visible to everyone in the area). */
export function visibleNav(items: NavItem[], roles: readonly Role[]): NavItem[] {
  return items.filter((item) => !item.roles || item.roles.some((r) => roles.includes(r)))
}

/** Case-page tabs for this case and viewer. */
export function panelsFor(c: Parameters<CasePanel['applies']>[0], audience: 'staff' | 'applicant'): CasePanel[] {
  return casePanels.filter((p) => (p.audience === 'both' || p.audience === audience) && p.applies(c))
}
