import type { NavItem } from '@/featureTypes'
export const staffNav: NavItem[] = [
  {
    to: '/staff/calendar',
    label: 'Calendar',
    icon: 'calendar',
    roles: ['intake', 'specialist', 'manager'],
  },
  {
    to: '/staff/field',
    label: 'Field tasks',
    icon: 'wrench',
    roles: ['field_worker', 'intake', 'specialist', 'manager'],
  },
]
export const adminNav: NavItem[] = [
  {
    to: '/admin/resources',
    label: 'Resources',
    icon: 'calendar',
    roles: ['manager', 'sysadmin'],
  },
]
export const residentNav: NavItem[] = [
  { to: '/map', label: 'Road issues map', icon: 'map' },
]
// Platform's public header currently has fixed links; expose this for the orchestrator to connect.
export const publicNav: NavItem[] = [
  { to: '/map', label: 'Road issues map', icon: 'map' },
]
