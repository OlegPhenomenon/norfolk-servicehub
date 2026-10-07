import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useLocation, useNavigate } from 'react-router'
import { api, isApiError } from '@/api/client'
import { ROLE_LABELS, type Persona } from '@/api/types'
import { PageContainer } from '@/layout/PageContainer'
import { Alert, Badge, Button, cn, EmptyState, ErrorAlert, LoadingState, PageHeader, useToast } from '@/ui'
import { DemoAuthenticator } from './DemoAuthenticator'
import { safeNext, withNext } from './next'
import { PERSONA_DESCRIPTIONS, PERSONA_ORDER } from './personas'
import { useMe, useRefreshMe } from './useMe'

/** `/demo` — one-click persona sign-in plus the live Demo authenticator. */
export function DemoPage() {
  const { data: me } = useMe()
  const refreshMe = useRefreshMe()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const toast = useToast()

  const personas = useQuery({
    queryKey: ['demo', 'personas'],
    queryFn: () => api.get<Persona[]>('/api/demo/personas'),
    staleTime: Infinity,
  })

  const login = useMutation({
    mutationFn: (persona: string) => api.post('/api/demo/login', { persona }),
    onSuccess: async (_data, persona) => {
      // Drop everything cached for the previous persona.
      qc.removeQueries({ predicate: (q) => q.queryKey[0] !== 'me' && q.queryKey[0] !== 'demo' })
      const fresh = await refreshMe()
      const isStaff = fresh.user?.kind === 'staff'
      const next = safeNext(location.search, isStaff ? '/staff' : '/my')
      if (isStaff && fresh.mfa_required) {
        void navigate(withNext('/login/totp', next))
      } else {
        toast.success(`Signed in as ${fresh.user?.name ?? persona}.`)
        void navigate(next)
      }
    },
    onError: (e) => toast.error(e, 'Could not sign in'),
  })

  if (personas.isPending) return <LoadingState label="Loading personas…" className="min-h-[50vh]" />
  if (personas.isError) {
    return (
      <PageContainer narrow>
        {isApiError(personas.error, 'not_found') ? (
          <EmptyState
            icon="lock"
            title="Demo personas are turned off"
            description="This installation runs without demo mode, so persona sign-in and the demo authenticator are disabled. Sign in with your account instead."
          />
        ) : (
          <ErrorAlert error={personas.error} onRetry={() => void personas.refetch()} />
        )}
      </PageContainer>
    )
  }

  const rank = (p: Persona) => {
    const i = PERSONA_ORDER.indexOf(p.persona)
    return i === -1 ? PERSONA_ORDER.length : i
  }
  const sorted = [...personas.data].sort((a, b) => rank(a) - rank(b))
  const residents = sorted.filter((p) => p.kind === 'resident')
  const staff = sorted.filter((p) => p.kind === 'staff')
  const currentPersona = me?.user?.persona_key ?? null

  return (
    <PageContainer>
      <PageHeader
        eyebrow="Demonstration"
        title="Choose who you are"
        description="Every persona is fictional. Sign in with one click, act, then switch persona to see the same request from the other side of the counter."
      />
      <div className="grid gap-10 lg:grid-cols-[1fr_22rem]">
        <div className="flex flex-col gap-10">
          <PersonaGroup title="Residents and businesses" hint="Ask for services, reply to the council, pay and download results." personas={residents} current={currentPersona} pending={login.isPending ? login.variables : undefined} onPick={(p) => login.mutate(p)} />
          <PersonaGroup
            title="Council staff"
            hint="Staff sign in with a second step: the code from the Demo authenticator on this page."
            personas={staff}
            current={currentPersona}
            pending={login.isPending ? login.variables : undefined}
            onPick={(p) => login.mutate(p)}
          />
        </div>
        <aside className="flex flex-col gap-4 lg:sticky lg:top-6 lg:self-start">
          <DemoAuthenticator />
          <Alert tone="info" title="Nothing here is real">
            Payments go through a test-mode checkout, emails land in DemoMail, and all data is reset regularly.
          </Alert>
        </aside>
      </div>
    </PageContainer>
  )
}

function PersonaGroup({ title, hint, personas, current, pending, onPick }: { title: string; hint: string; personas: Persona[]; current: string | null; pending?: string; onPick: (persona: string) => void }) {
  if (personas.length === 0) return null
  return (
    <section aria-labelledby={`group-${title}`}>
      <h2 id={`group-${title}`} className="font-serif text-2xl font-semibold">
        {title}
      </h2>
      <p className="mt-1 text-muted">{hint}</p>
      <ul className="mt-5 grid gap-4 sm:grid-cols-2">
        {personas.map((p) => {
          const isCurrent = p.persona === current
          const first = p.name.split(' ')[0] ?? p.name
          return (
            <li key={p.persona} className={cn('flex flex-col rounded-[var(--radius-card)] border bg-surface p-5 shadow-[var(--shadow-card)]', isCurrent ? 'border-primary ring-2 ring-primary-100' : 'border-line')}>
              <div className="flex items-start gap-3">
                <span aria-hidden="true" className={cn('flex size-11 shrink-0 items-center justify-center rounded-full font-bold text-white', p.kind === 'staff' ? 'bg-pine' : 'bg-primary')}>
                  {p.name
                    .split(/\s+/)
                    .slice(0, 2)
                    .map((s) => s[0])
                    .join('')}
                </span>
                <div className="min-w-0">
                  <h3 className="font-semibold leading-snug">{p.name}</h3>
                  <p className="text-sm text-muted">{p.organisation ?? p.job_title ?? (p.kind === 'resident' ? 'Resident' : 'Council staff')}</p>
                </div>
                {isCurrent ? (
                  <Badge tone="primary" className="ml-auto">
                    Signed in
                  </Badge>
                ) : null}
              </div>
              {p.roles.length > 0 ? (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {p.roles.map((r) => (
                    <Badge key={r} tone="accent">
                      {ROLE_LABELS[r] ?? r}
                    </Badge>
                  ))}
                </div>
              ) : null}
              {PERSONA_DESCRIPTIONS[p.persona] ? <p className="mt-3 flex-1 text-[0.95rem] text-ink/85">{PERSONA_DESCRIPTIONS[p.persona]}</p> : <div className="flex-1" />}
              <Button className="mt-4" variant={isCurrent ? 'primary' : 'secondary'} iconRight="arrowRight" loading={pending === p.persona} disabled={!!pending} onClick={() => onPick(p.persona)} fullWidth>
                {isCurrent ? `Continue as ${first}` : `Sign in as ${first}`}
              </Button>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
