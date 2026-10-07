import type { RouteObject } from 'react-router'
export const publicRoutes: RouteObject[] = [
  {
    path: 'map',
    lazy: async () => ({
      Component: (await import('./RoadMapPage')).RoadMapPage,
    }),
  },
]
export const residentRoutes: RouteObject[] = []
export const staffRoutes: RouteObject[] = [
  {
    path: 'calendar',
    lazy: async () => ({
      Component: (await import('./CalendarPage')).CalendarPage,
    }),
  },
  {
    path: 'field',
    lazy: async () => ({ Component: (await import('./FieldPages')).FieldPage }),
  },
  {
    path: 'field/:id',
    lazy: async () => ({
      Component: (await import('./FieldPages')).FieldTaskPage,
    }),
  },
]
export const adminRoutes: RouteObject[] = [
  {
    path: 'resources',
    lazy: async () => ({
      Component: (await import('./ResourcesPage')).ResourcesPage,
    }),
  },
]
