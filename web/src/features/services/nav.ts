// OWNER: services
import type { NavItem } from '@/featureTypes'

/*
 * Navigation entries of the "services" feature. `to` is absolute; `roles` limits visibility.
 *   { to: '/staff/bookings', label: 'Bookings', icon: 'calendar', roles: ['intake', 'manager'] }
 */

/** Sidebar of the staff workspace. */
export const staffNav: NavItem[] = []

/** Sidebar of the admin area. */
export const adminNav: NavItem[] = []

/** Menu of the resident area (`/my`); `roles` is ignored there. */
export const residentNav: NavItem[] = []
