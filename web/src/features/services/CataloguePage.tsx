import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { hasRole, useMe } from '@/auth/useMe'
import { PageContainer } from '@/layout/PageContainer'
import { Alert, Badge, Button, ButtonLink, Card, EmptyState, ErrorAlert, Field, Money, PageHeader, QueryView, Steps, TextInput } from '@/ui'
import { DEMO_SCHEDULE_NOTE, firstSentence } from './copy'
import type { Catalogue, ServiceDetail } from './types'

export function CataloguePage() {
  const [q, setQ] = useState('')
  const [category, setCategory] = useState('')
  const data = useQuery({ queryKey: ['services', 'catalogue', q, category], queryFn: ({ signal }) => api.get<Catalogue>('/api/public/services', { query: { q, category: category || undefined }, signal }) })
  return <PageContainer>
    <PageHeader title="Council services" description="Find a service, prepare your documents and follow your request to a result." />
    <Field label="Search services" required><TextInput type="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Try hall, pothole or planning certificate" /></Field>
    <QueryView query={data}>{(catalogue) => <>
      <div className="my-5 flex flex-wrap gap-2" aria-label="Service categories">
        <Button variant={!category ? 'primary' : 'secondary'} aria-pressed={!category} onClick={() => setCategory('')}>All services</Button>
        {catalogue.categories.map((c) => <Button key={c} variant={category === c ? 'primary' : 'secondary'} aria-pressed={category === c} onClick={() => setCategory(c)}>{c}</Button>)}
      </div>
      <p className="mb-3 text-muted" aria-live="polite">{catalogue.items.length} services found</p>
      <div className="grid gap-5 md:grid-cols-2 lg:grid-cols-3">
        {catalogue.items.map((s) => <Card key={s.id} className="flex h-full flex-col [&>div]:flex-1" title={<Link className="link" to={`/services/${s.slug}`}>{s.name}</Link>} footer={<ButtonLink to={`/services/${s.slug}`} variant="secondary">View service</ButtonLink>}>
          <Badge>{s.department}</Badge>
          <p className="mt-3">{firstSentence(s.summary, 140)}</p>
          <p className="mt-3 text-sm text-muted">{firstSentence(s.price_note, 140) || 'Council will advise if a fee applies.'}</p>
        </Card>)}
      </div>
      {!catalogue.items.length && <EmptyState title="No matching services" description="Try a different word or choose all categories." />}
      <p className="mt-8 border-t border-line pt-4 text-sm text-muted">{DEMO_SCHEDULE_NOTE}</p>
    </>}</QueryView>
  </PageContainer>
}

export function ServicePage() {
  const { slug } = useParams()
  const navigate = useNavigate()
  const me = useMe()
  const data = useQuery({ queryKey: ['services', 'service', slug], queryFn: () => api.get<ServiceDetail>(`/api/public/services/${slug}`) })
  const start = useMutation({ mutationFn: () => api.post<{ id: number }>(`/api/services/${slug}/drafts`, {}), onSuccess: (d) => navigate(`/my/drafts/${d.id}`) })
  return <PageContainer><QueryView query={data}>{({ service, definition: def, prices }) => {
    const lead = firstSentence(def.summary)
    // Published definitions from before conditions were introduced keep their venue guidance.
    const listedConditions = (def.conditions ?? []).map((text) => text.trim()).filter(Boolean)
    const conditions = listedConditions.length ? listedConditions : service.module === 'venue_booking' ? def.summary.slice(lead.length).trim().split(/(?<=[.!?])\s+/).filter(Boolean) : []
    return <>
      <PageHeader title={service.name} eyebrow={service.department} description={lead} breadcrumbs={[{ label: 'Services', to: '/services' }, { label: service.name }]} />
      <div className="grid gap-6 lg:grid-cols-[2fr_1fr]">
        <div className="space-y-6">
          <Card title="What you get"><p>{def.outcome}</p></Card>
          <Card title="Who can apply"><p>{def.who_can_apply}</p></Card>
          {!!conditions.length && <Card title="Conditions"><ul className="list-disc space-y-2 pl-5">{conditions.map((condition, i) => <li key={i}>{condition}</li>)}</ul></Card>}
          <Card title="Documents to prepare">{def.documents.length ? <ul className="list-disc space-y-2 pl-5">{def.documents.map((d) => <li key={d.key}>{d.label}{d.required ? ' — required' : ' — optional'}</li>)}</ul> : <p>No supporting documents are needed.</p>}</Card>
          <Card title="How the price is calculated">
            <p>{def.price_note}</p>
            {prices.map((p) => <p key={p.code} className="flex flex-wrap justify-between gap-2 border-b border-line py-2"><span>{p.name.replace(/\s*\(demo schedule[^)]*\)/, '')}</span><span><Money cents={p.amount_cents} /> per {p.unit}</span></p>)}
          </Card>
        </div>
        <div className="space-y-6">
          <Card title="What happens next"><Steps steps={def.workflow.steps.map((s) => ({ key: s.key, label: s.applicant_label || s.label }))} /></Card>
          {me.data?.user?.kind === 'staff'
            ? <Card title="Assisted request">
              {hasRole(me.data, 'intake')
                ? <><p className="mb-4">Record this service for an applicant who contacted Council by phone, post, email or in person.</p><ButtonLink to={`/staff/intake?service=${encodeURIComponent(service.slug)}`} fullWidth>Record assisted request</ButtonLink></>
                : <p>Residents and businesses apply online. Intake officers record requests received by phone, post, email or in person.</p>}
            </Card>
            : <Card title="Start your request">
              {me.data?.user ? <Button fullWidth loading={start.isPending} onClick={() => start.mutate()}>Start request</Button> : <><ButtonLink to="/login" fullWidth>Sign in to start</ButtonLink>{me.data?.demo_mode && <p className="mt-3"><Link className="link" to="/demo">Try a fictional demo persona</Link></p>}</>}
              {start.error && <ErrorAlert error={start.error} />}
              <Alert title="Request first, decision later">{service.module === 'venue_booking' ? 'Your booking is confirmed separately after payment and Council review.' : 'Council will review your submitted request and let you know the outcome.'}</Alert>
            </Card>}
        </div>
      </div>
      <p className="mt-8 border-t border-line pt-4 text-sm text-muted">{DEMO_SCHEDULE_NOTE}</p>
    </>
  }}</QueryView></PageContainer>
}
