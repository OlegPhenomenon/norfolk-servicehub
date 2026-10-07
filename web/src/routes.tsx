/**
 * Route assembly (platform-owned). Areas and their layouts:
 *   /         PublicLayout    — home, sign-in, /demo, /mock/mail + features' publicRoutes
 *   /my       ResidentLayout  — RequireAuth + features' residentRoutes
 *   /staff    StaffLayout     — RequireStaff (TOTP passed) + features' staffRoutes
 *   /admin    AdminLayout     — RequireStaff(sysadmin|manager) + features' adminRoutes
 * Feature routes are added in `features/<module>/routes.tsx`, never here.
 */
import { createBrowserRouter, type RouteObject } from 'react-router'
import { DemoPage } from './auth/DemoPage'
import { LoginPage } from './auth/LoginPage'
import { RegisterPage } from './auth/RegisterPage'
import { TotpEnrolPage } from './auth/TotpEnrolPage'
import { TotpPage } from './auth/TotpPage'
import { AdminLayout } from './layout/AdminLayout'
import { PublicLayout } from './layout/PublicLayout'
import { ResidentLayout } from './layout/ResidentLayout'
import { Root } from './layout/Root'
import { StaffLayout } from './layout/StaffLayout'
import { AdminHomePage, ResidentHomePage, StaffHomePage } from './pages/AreaHomePages'
import { DemoMailPage } from './pages/DemoMailPage'
import { NotFoundPage, RouteErrorPage } from './pages/ErrorPages'
import { HomePage } from './pages/HomePage'
import { adminRoutes, publicRoutes, residentRoutes, staffRoutes } from './registry'
import { LoadingState } from './ui'

/** Area children: the platform overview page unless a feature provides an index route, then a 404 catch-all. */
function areaChildren(featureRoutes: RouteObject[], Overview: () => React.JSX.Element): RouteObject[] {
  const hasIndex = featureRoutes.some((r) => r.index)
  return [
    {
      errorElement: <RouteErrorPage />,
      children: [...(hasIndex ? [] : [{ index: true, Component: Overview }]), ...featureRoutes, { path: '*', Component: NotFoundPage }],
    },
  ]
}

export const routes: RouteObject[] = [
  {
    Component: Root,
    hydrateFallbackElement: <LoadingState label="Loading page…" />,
    children: [
      {
        path: '/',
        Component: PublicLayout,
        children: [
          {
            errorElement: <RouteErrorPage />,
            children: [
              { index: true, Component: HomePage },
              { path: 'login', Component: LoginPage },
              { path: 'login/totp', Component: TotpPage },
              { path: 'register', Component: RegisterPage },
              { path: 'demo', Component: DemoPage },
              { path: 'mock/mail', Component: DemoMailPage },
              // Staff 2FA enrolment lives outside the /staff guard (staff without TOTP must reach it).
              { path: 'staff/settings/2fa', Component: TotpEnrolPage },
              ...publicRoutes,
              { path: '*', Component: NotFoundPage },
            ],
          },
        ],
      },
      { path: '/my', Component: ResidentLayout, children: areaChildren(residentRoutes, ResidentHomePage) },
      { path: '/staff', Component: StaffLayout, children: areaChildren(staffRoutes, StaffHomePage) },
      { path: '/admin', Component: AdminLayout, children: areaChildren(adminRoutes, AdminHomePage) },
    ],
  },
]

export const router = createBrowserRouter(routes)
