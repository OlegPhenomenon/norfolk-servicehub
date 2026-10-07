import type { NavItem } from '@/featureTypes'
export const staffNav: NavItem[] = [
  { to: '/staff/finance', label: 'Finance', icon: 'coins', roles: ['finance'], end: true },
  { to: '/staff/finance/statements', label: 'Bank statements', icon: 'upload', roles: ['finance'] },
  { to: '/staff/finance/unmatched', label: 'Unmatched transfers', icon: 'receipt', roles: ['finance'] },
  { to: '/staff/finance/deposits', label: 'Refundable bonds', icon: 'shield', roles: ['finance'] },
  { to: '/staff/finance/refunds', label: 'Refunds', icon: 'coins', roles: ['finance'] },
  { to: '/admin/prices', label: 'Prices', icon: 'coins', roles: ['finance'] },
]
export const adminNav: NavItem[] = [{ to: '/admin/prices', label: 'Prices', icon: 'coins', roles: ['finance', 'sysadmin'] }]
export const residentNav: NavItem[] = []
