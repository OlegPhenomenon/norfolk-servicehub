import type { ReactNode } from 'react'
import { Navigate, useLocation } from 'react-router'
import { ErrorAlert, LoadingState } from '@/ui'
import { withNext } from './next'
import { useMe } from './useMe'

/** Renders children only for a signed-in user; otherwise redirects to `/login?next=…`. */
export function RequireAuth({ children }: { children: ReactNode }) {
  const { data: me, isPending, error, refetch } = useMe()
  const location = useLocation()
  if (isPending) return <LoadingState label="Checking your session…" className="min-h-[50vh]" />
  if (error || !me) return <div className="mx-auto max-w-xl p-6"><ErrorAlert error={error} onRetry={() => void refetch()} /></div>
  if (!me.user) return <Navigate to={withNext('/login', location.pathname + location.search)} replace />
  if (!me.mfa_required && me.password_change_required) return <Navigate to="/login/change-password" replace />
  return <>{children}</>
}
