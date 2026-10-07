import { RequireStaff } from '@/auth/RequireStaff'
import type { NavItem } from '@/featureTypes'
import { adminNav } from '@/registry'
import { WorkspaceShell } from './WorkspaceShell'

/** Roles that may enter `/admin`. Individual pages and the API check finer permissions. */
const ADMIN_ROLES = ['sysadmin', 'manager'] as const

const OVERVIEW: NavItem = { to: '/admin', label: 'Overview', icon: 'home', end: true }
const FOOTER: NavItem[] = [{ to: '/staff', label: 'Staff workspace', icon: 'clipboard', end: true }]

/** `/admin` — configuration (sysadmin; managers for authority grants). Sidebar = Overview + `adminNav`. */
export function AdminLayout() {
  return (
    <RequireStaff roles={ADMIN_ROLES}>
      <WorkspaceShell areaName="Administration" nav={[OVERVIEW, ...adminNav]} footerNav={FOOTER} />
    </RequireStaff>
  )
}
