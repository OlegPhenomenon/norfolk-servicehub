import type { RouteObject } from 'react-router'
export const publicRoutes: RouteObject[] = [
  { path: 'notices', lazy: async () => ({ Component: (await import('./NoticesPages')).NoticesPage }) },
  { path: 'notices/:id', lazy: async () => ({ Component: (await import('./NoticesPages')).NoticePage }) },
]
export const residentRoutes: RouteObject[] = [{ path: 'projects/:id', lazy: async () => ({ Component: (await import('./ProjectPage')).ProjectPage }) }]
export const staffRoutes: RouteObject[] = [
  { path: 'projects/:id', lazy: async () => ({ Component: (await import('./ProjectPage')).ProjectPage }) },
  { path: 'exhibitions', lazy: async () => ({ Component: (await import('./ExhibitionsPages')).ExhibitionsPage }) },
  { path: 'exhibitions/:id', lazy: async () => ({ Component: (await import('./ExhibitionsPages')).ExhibitionPage }) },
]
export const adminRoutes: RouteObject[] = []
