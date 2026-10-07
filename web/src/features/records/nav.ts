import type { NavItem } from '@/featureTypes'
export const staffNav: NavItem[] = [
  { to: '/staff/dashboard', label: 'Manager dashboard', icon: 'chart', roles: ['manager'] },
  { to: '/staff/records', label: 'Records', icon: 'archive', roles: ['manager'] },
  { to: '/staff/authority', label: 'Decision authority', icon: 'key', roles: ['manager'] },
]
export const adminNav: NavItem[] = [
  { to: '/admin/users', label: 'Users', icon: 'users', roles: ['sysadmin'] },
  { to: '/admin/settings', label: 'Notification settings', icon: 'settings', roles: ['sysadmin'] },
  { to: '/admin/deliveries', label: 'Notification deliveries', icon: 'mail', roles: ['sysadmin'] },
  { to: '/admin/integrations', label: 'Integrations', icon: 'external', roles: ['sysadmin'] },
  { to: '/admin/legacy-import', label: 'Legacy import', icon: 'upload', roles: ['sysadmin'] },
  { to: '/admin/backups', label: 'Backups', icon: 'shield', roles: ['sysadmin'] },
  { to: '/admin/retention', label: 'Retention rules', icon: 'archive', roles: ['sysadmin'] },
]
export const residentNav: NavItem[] = [{ to: '/my/organisation', label: 'Organisation', icon: 'building' }]
