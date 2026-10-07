// OWNER: finance
import type { RouteObject } from 'react-router'

/*
 * Routes of the "finance" feature. Paths are RELATIVE to the area they are mounted in (no leading slash).
 * Prefer lazy routes so each feature is its own chunk:
 *
 *   { path: 'bookings/:id', lazy: async () => ({ Component: (await import('./pages/BookingPage')).BookingPage }) }
 *
 * An `index: true` route in residentRoutes/staffRoutes/adminRoutes replaces the platform's placeholder overview page.
 */

/** Children of the public layout at `/`, e.g. `services`, `services/:slug`, `notices`. */
export const publicRoutes: RouteObject[] = []

/** Children of `/my` (signed-in residents and businesses). */
export const residentRoutes: RouteObject[] = []

/** Children of `/staff` (staff who passed 2FA). */
export const staffRoutes: RouteObject[] = []

/** Children of `/admin` (sysadmin / manager). */
export const adminRoutes: RouteObject[] = []
