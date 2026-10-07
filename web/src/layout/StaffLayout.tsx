import { RequireStaff } from '@/auth/RequireStaff'
import type { NavItem } from '@/featureTypes'
import { staffNav } from '@/registry'
import { WorkspaceShell } from './WorkspaceShell'

const OVERVIEW: NavItem = { to: '/staff', label: 'Overview', icon: 'home', end: true }
const FOOTER: NavItem[] = [
  { to: '/admin', label: 'Administration', icon: 'settings', roles: ['sysadmin', 'manager'] },
  { to: '/staff/settings/2fa', label: 'Two-step verification', icon: 'key' },
]

/** `/staff` — council staff who passed TOTP. Sidebar = Overview + `staffNav` from features, filtered by role. */
export function StaffLayout() {
  return (
    <RequireStaff>
      <WorkspaceShell areaName="Staff workspace" nav={[OVERVIEW, ...staffNav]} footerNav={FOOTER} />
    </RequireStaff>
  )
}
