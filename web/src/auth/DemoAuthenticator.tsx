import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import type { AuthenticatorCode } from '@/api/types'
import { Button, cn, ErrorAlert, Icon, Spinner } from '@/ui'

const AUTHENTICATOR_QUERY_KEY = ['demo', 'authenticator'] as const

/** Polls `/api/demo/authenticator` every 5 s and ticks a local countdown every second. */
function useAuthenticatorCodes() {
  const query = useQuery({
    queryKey: AUTHENTICATOR_QUERY_KEY,
    queryFn: () => api.get<AuthenticatorCode[]>('/api/demo/authenticator'),
    refetchInterval: 5_000,
    staleTime: 0,
  })
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1_000)
    return () => window.clearInterval(t)
  }, [])
  const elapsed = query.dataUpdatedAt ? Math.max(0, Math.floor((now - query.dataUpdatedAt) / 1000)) : 0
  return { query, elapsed }
}

function Countdown({ seconds }: { seconds: number }) {
  const pct = Math.max(0, Math.min(1, seconds / 30))
  const r = 9
  const c = 2 * Math.PI * r
  return (
    <span className="flex items-center gap-1.5 text-sm text-muted tabular-nums" title="Seconds until the code changes">
      <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true" className="-rotate-90">
        <circle cx="12" cy="12" r={r} fill="none" stroke="currentColor" strokeOpacity="0.2" strokeWidth="3" />
        <circle cx="12" cy="12" r={r} fill="none" stroke="currentColor" strokeWidth="3" strokeDasharray={c} strokeDashoffset={c * (1 - pct)} className={cn(seconds <= 5 ? 'text-warning' : 'text-pine')} />
      </svg>
      {seconds}s
    </span>
  )
}

/**
 * The "Demo authenticator": stands in for a phone authenticator app in demo mode.
 * - Without `persona`: lists codes for every staff persona (on /demo).
 * - With `persona`: shows just that persona's code, with a "Use code" button calling `onUse`.
 */
export function DemoAuthenticator({ persona, onUse, className }: { persona?: string | null; onUse?: (code: string) => void; className?: string }) {
  const { query, elapsed } = useAuthenticatorCodes()
  const rows = (query.data ?? []).filter((r) => !persona || r.persona === persona)

  return (
    <section aria-labelledby="demo-auth-title" className={cn('rounded-2xl border border-pine/25 bg-pine-50/70 p-5', className)}>
      <div className="flex items-center gap-2.5">
        <span className="flex size-9 items-center justify-center rounded-lg bg-pine text-white">
          <Icon name="key" size={18} />
        </span>
        <div>
          <h2 id="demo-auth-title" className="font-semibold leading-tight">
            Demo authenticator
          </h2>
          <p className="text-sm text-muted">Stands in for the staff member’s phone app. Demo mode only.</p>
        </div>
      </div>
      {query.isPending ? (
        <div className="flex justify-center py-6 text-pine">
          <Spinner label="Loading codes" />
        </div>
      ) : query.isError ? (
        <ErrorAlert error={query.error} title="Codes unavailable" className="mt-4" />
      ) : rows.length === 0 ? (
        <p className="mt-4 text-sm text-muted">No code for this account.</p>
      ) : (
        <ul className="mt-4 divide-y divide-pine/15 rounded-xl border border-pine/20 bg-surface">
          {rows.map((r) => {
            const left = Math.max(0, r.seconds_left - elapsed)
            return (
              <li key={r.persona} className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
                <span className="min-w-0 flex-1">
                  <span className="block text-sm text-muted">{r.name}</span>
                  <span className={cn('font-mono text-2xl font-semibold tracking-[0.12em] tabular-nums', left === 0 && 'opacity-40')} aria-label={`Code ${r.code.split('').join(' ')}`}>
                    {r.code.length === 6 ? `${r.code.slice(0, 3)} ${r.code.slice(3)}` : r.code}
                  </span>
                </span>
                <Countdown seconds={left} />
                {onUse ? (
                  <Button size="sm" variant="accent" icon="check" onClick={() => onUse(r.code)} disabled={left === 0}>
                    Use code
                  </Button>
                ) : null}
              </li>
            )
          })}
        </ul>
      )}
    </section>
  )
}
