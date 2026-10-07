import type { RouteObject } from 'react-router'
export const publicRoutes: RouteObject[] = [
  { path: 'services', lazy: async () => ({ Component: (await import('./CataloguePage')).CataloguePage }) },
  { path: 'services/:slug', lazy: async () => ({ Component: (await import('./CataloguePage')).ServicePage }) },
]
export const residentRoutes: RouteObject[] = []
export const staffRoutes: RouteObject[] = []
export const adminRoutes: RouteObject[] = [
  { path: 'services', lazy: async () => ({ Component: (await import('./BuilderPage')).ServiceAdminPage }) },
  { path: 'services/new', lazy: async () => ({ Component: (await import('./BuilderPage')).NewServicePage }) },
  { path: 'services/:id', lazy: async () => ({ Component: (await import('./BuilderPage')).ServiceEditorPage }) },
  { path: 'service-imports', lazy: async () => ({ Component: (await import('./ImportsPage')).ImportsPage }) },
]
