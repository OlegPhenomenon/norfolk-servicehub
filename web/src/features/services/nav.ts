import type { NavItem } from '@/featureTypes'
export const staffNav: NavItem[] = []
export const adminNav: NavItem[] = [{ to: '/admin/services', label: 'Services', icon: 'clipboard', roles: ['sysadmin'] }, { to: '/admin/service-imports', label: 'Bulk import', icon: 'upload', roles: ['sysadmin'] }]
export const residentNav: NavItem[] = []
export const publicNav: NavItem[] = [{ to: "/services", label: "Services", icon: "clipboard" }]
