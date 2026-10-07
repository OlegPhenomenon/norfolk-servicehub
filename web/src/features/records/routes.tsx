import type { RouteObject } from 'react-router'
import { RequireStaff } from '@/auth/RequireStaff'
import { AuthorityPage, BackupsPage, DeliveriesPage, RetentionPage, SettingsPage, UsersPage } from './AdminPages'
import { DashboardPage, MetricPage } from './DashboardPage'
import { IntegrationsPage } from './IntegrationsPage'
import { LegacyImportPage } from './LegacyImportPage'
import { InvitePage, OrganisationPage } from './OrganisationPage'
import { RecordsPage } from './RecordsPage'
export const publicRoutes: RouteObject[] = []
export const residentRoutes: RouteObject[] = [{ path: 'organisation', Component: OrganisationPage }, { path: 'invites/:token', Component: InvitePage }]
export const staffRoutes: RouteObject[] = [
  { path: 'dashboard', element: <RequireStaff roles={['manager']}><DashboardPage /></RequireStaff> },
  { path: 'dashboard/metrics/:metric', element: <RequireStaff roles={['manager']}><MetricPage /></RequireStaff> },
  { path: 'records', element: <RequireStaff roles={['manager']}><RecordsPage /></RequireStaff> },
  { path: 'authority', element: <RequireStaff roles={['manager']}><AuthorityPage /></RequireStaff> },
]
export const adminRoutes: RouteObject[] = [
  { path: 'users', element: <RequireStaff roles={['sysadmin']}><UsersPage /></RequireStaff> },
  { path: 'settings', element: <RequireStaff roles={['sysadmin']}><SettingsPage /></RequireStaff> },
  { path: 'deliveries', element: <RequireStaff roles={['sysadmin']}><DeliveriesPage /></RequireStaff> },
  { path: 'integrations', element: <RequireStaff roles={['sysadmin']}><IntegrationsPage /></RequireStaff> },
  { path: 'legacy-import', element: <RequireStaff roles={['sysadmin']}><LegacyImportPage /></RequireStaff> },
  { path: 'backups', element: <RequireStaff roles={['sysadmin']}><BackupsPage /></RequireStaff> },
  { path: 'retention', element: <RequireStaff roles={['sysadmin']}><RetentionPage /></RequireStaff> },
]
