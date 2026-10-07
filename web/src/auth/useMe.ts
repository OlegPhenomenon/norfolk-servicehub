import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, setCsrfToken } from '../api/client'
import type { Me, Role } from '../api/types'

export const ME_QUERY_KEY = ['me'] as const

async function fetchMe(): Promise<Me> {
  const me = await api.get<Me>('/api/me')
  setCsrfToken(me.csrf_token)
  return me
}

/**
 * Current session: `{ user, roles, csrf_token, mfa_required, demo_mode, next_reset_at }`.
 *
 *   const { data: me, isPending } = useMe()
 *   if (me?.user?.kind === 'staff') …
 *
 * Use `hasRole(me, 'finance')` / `hasAnyRole(me, [...])` for UI decisions. The server always re-checks.
 */
export function useMe() {
  return useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, staleTime: 60_000 })
}

/** Re-load `/api/me` (call after login, logout, TOTP, persona switch). Resolves with the new session. */
export function useRefreshMe() {
  const qc = useQueryClient()
  return async (): Promise<Me> => {
    const me = await qc.fetchQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, staleTime: 0 })
    return me
  }
}

export function hasRole(me: Me | undefined, role: Role): boolean {
  return !!me?.user && me.roles.includes(role)
}

export function hasAnyRole(me: Me | undefined, roles: readonly Role[]): boolean {
  return !!me?.user && roles.some((r) => me.roles.includes(r))
}

/** A staff session that has passed TOTP. */
export function isStaffReady(me: Me | undefined): boolean {
  return me?.user?.kind === 'staff' && !me.mfa_required
}
