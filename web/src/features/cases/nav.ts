import type { NavItem } from '@/featureTypes'
export const staffNav: NavItem[] = [{ to: '/staff', label: 'Home', icon: 'home', end: true }, { to: '/staff/cases', label: 'Cases', icon: 'folder' }, { to: '/staff/intake', label: 'Assisted intake', icon: 'phone', roles: ['intake'] }]
export const adminNav: NavItem[] = []
export const residentNav: NavItem[] = [{ to: '/my', label: 'My requests', icon: 'folder', end: true }]
