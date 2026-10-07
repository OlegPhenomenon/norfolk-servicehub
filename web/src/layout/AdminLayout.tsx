import { RequireStaff } from '@/auth/RequireStaff'
import { useLocation } from 'react-router'
import type { NavItem } from '@/featureTypes'
import { adminNav } from '@/registry'
import { WorkspaceShell } from './WorkspaceShell'

/** Default admin roles; finance may also enter the price page. APIs check finer permissions. */
const ADMIN_ROLES = ['sysadmin', 'manager'] as const

const OVERVIEW: NavItem = { to: '/admin', label: 'Overview', icon: 'home', end: true }
const FOOTER: NavItem[] = [{ to: '/staff', label: 'Staff workspace', icon: 'clipboard', end: true }]

/** `/admin` — configuration (sysadmin; managers for authority grants). Sidebar = Overview + `adminNav`. */
export function AdminLayout() {
  const { pathname } = useLocation()
  const roles = pathname.replace(/\/+$/, '') === '/admin/prices' ? [...ADMIN_ROLES, 'finance'] as const : ADMIN_ROLES
  return (
    <RequireStaff roles={roles}>
      <WorkspaceShell areaName="Administration" nav={[OVERVIEW, ...adminNav]} footerNav={FOOTER} />
    </RequireStaff>
  )
}
