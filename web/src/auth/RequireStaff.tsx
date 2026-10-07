import type { ReactNode } from 'react'
import { Navigate, useLocation } from 'react-router'
import type { Role } from '@/api/types'
import { PageContainer } from '@/layout/PageContainer'
import { ButtonLink, EmptyState, ErrorAlert, LoadingState } from '@/ui'
import { withNext } from './next'
import { hasAnyRole, useMe } from './useMe'

/**
 * Staff-only gate:
 * - anonymous → `/login?next=…`
 * - resident → "staff only" message
 * - staff without TOTP set up → `/staff/settings/2fa`
 * - staff who has not entered the code this session → `/login/totp?next=…`
 * - `roles` given and none held → "no access" message
 */
export function RequireStaff({ roles, children }: { roles?: readonly Role[]; children: ReactNode }) {
  const { data: me, isPending, error, refetch } = useMe()
  const location = useLocation()
  const here = location.pathname + location.search

  if (isPending) return <LoadingState label="Checking your session…" className="min-h-[50vh]" />
  if (error || !me) return <div className="mx-auto max-w-xl p-6"><ErrorAlert error={error} onRetry={() => void refetch()} /></div>
  if (!me.user) return <Navigate to={withNext('/login', here)} replace />
  if (me.user.kind !== 'staff') {
    return (
      <PageContainer narrow>
        <EmptyState icon="lock" title="This area is for council staff" description="You are signed in as a resident. Your requests are in My requests." action={<ButtonLink to="/my">Go to My requests</ButtonLink>} />
      </PageContainer>
    )
  }
  if (me.mfa_required && !me.user.totp_enabled) return <Navigate to={withNext('/staff/settings/2fa', here)} replace />
  if (me.mfa_required) return <Navigate to={withNext('/login/totp', here)} replace />
  if (me.password_change_required) return <Navigate to="/login/change-password" replace />
  if (roles && !hasAnyRole(me, roles)) {
    return (
      <PageContainer narrow>
        <EmptyState icon="lock" title="You don't have access to this area" description="Ask a manager or the systems administrator if you need it." action={<ButtonLink to="/staff">Back to the staff workspace</ButtonLink>} />
      </PageContainer>
    )
  }
  return <>{children}</>
}
