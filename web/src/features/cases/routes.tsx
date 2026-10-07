import type { RouteObject } from 'react-router'
export const publicRoutes: RouteObject[] = []
export const residentRoutes: RouteObject[] = [
  { index: true, lazy: async () => ({ Component: (await import('./ListsPage')).MyRequestsPage }) },
  { path: 'drafts/:id', lazy: async () => ({ Component: (await import('./DraftPage')).DraftPage }) },
  { path: 'cases/:id', lazy: async () => ({ Component: (await import('./CasePage')).ResidentCasePage }) },
]
export const staffRoutes: RouteObject[] = [
  { index: true, lazy: async () => ({ Component: (await import('./ListsPage')).StaffHomePage }) },
  { path: 'cases', lazy: async () => ({ Component: (await import('./ListsPage')).StaffCaseList }) },
  { path: 'cases/:id', lazy: async () => ({ Component: (await import('./CasePage')).StaffCasePage }) },
  { path: 'intake', lazy: async () => ({ Component: (await import('./IntakePage')).IntakePage }) },
]
export const adminRoutes: RouteObject[] = []
