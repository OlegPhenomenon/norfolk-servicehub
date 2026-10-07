import { useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Card, Field, TextInput, PageHeader, QueryView, Table, humanize, norfolkToday } from '@/ui'
import type { Dashboard, RecordCase } from './types'
import { CasesTable } from './CasesTable'
const LABELS: Record<string, string> = { received: 'Received in period', open: 'Open requests', waiting_on_applicant: 'Waiting on applicant', completed: 'Completed', refused: 'Refused', withdrawn: 'Withdrawn', cancelled: 'Cancelled', reopened: 'Reopened', unassigned: 'Unassigned', overdue: 'Overdue', due_soon: 'Due in next 3 business days' }
function metricLink(metric: string, period: string, extra?: Record<string, string>) {
  const search = new URLSearchParams(period)
  for (const [key, value] of Object.entries(extra ?? {})) search.set(key, value)
  return `/staff/dashboard/metrics/${metric}?${search}`
}
export function DashboardPage() {
  const [parameters] = useSearchParams()
  const [from, setFrom] = useState(() => { if (parameters.get('from')) return parameters.get('from')!; const day = new Date(`${norfolkToday()}T12:00:00Z`); day.setUTCDate(day.getUTCDate() - 30); return day.toISOString().slice(0, 10) })
  const [to, setTo] = useState(() => parameters.get('to') ?? norfolkToday())
  const period = new URLSearchParams({ from, to }).toString()
  const q = useQuery({ queryKey: ['records', 'dashboard', period], queryFn: () => api.get<Dashboard>(`/api/staff/dashboard?${period}`) })
  const errors = isApiError(q.error) ? q.error.fields : {}
  return <div className="space-y-6"><PageHeader title="Manager dashboard" description="Current workload and outcomes in the selected period. Reopened requests are shown separately from completed requests." />
    <Card title="Reporting period"><div className="grid gap-4 sm:grid-cols-2"><Field label="From (Norfolk date)" required error={errors.from}><TextInput type="date" value={from} onChange={e => setFrom(e.target.value)} /></Field><Field label="To (inclusive)" required error={errors.to}><TextInput type="date" value={to} onChange={e => setTo(e.target.value)} /></Field></div></Card>
    <QueryView query={q}>{d => <>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">{Object.entries(d.metrics).map(([metric, count]) => <Link className="rounded-xl border border-line bg-surface p-5 hover:bg-sunken" key={metric} to={metricLink(metric, period)}><span className="block text-muted">{LABELS[metric]}</span><span className="text-3xl font-semibold">{count}</span></Link>)}</div>
      <Card title="By service" padded={false}><Table caption="Service outcomes" rows={d.services} rowKey={s => s.service_id} columns={[
        { key: 'service', header: 'Service', cell: s => s.service_name },
        ...Object.keys(d.metrics).map(metric => ({ key: metric, header: LABELS[metric], cell: (s: Dashboard['services'][number]) => <Link className="link" to={metricLink(metric, period, { service_id: String(s.service_id) })}>{s.metrics[metric]}</Link> })),
        { key: 'median', header: 'Median days to complete', cell: s => s.median_days == null ? '—' : <Link className="link" to={metricLink('completed', period, { service_id: String(s.service_id) })}>{s.median_days.toFixed(1)}</Link> },
      ]} /></Card>
      <Card title="Staff workload" padded={false}><Table caption="Owned open requests" rows={d.workload} rowKey={s => s.user_id} columns={[
        { key: 'staff', header: 'Staff member', cell: s => s.name },
        { key: 'open', header: 'Open', cell: s => <Link className="link" to={metricLink('open', period, { owner_id: String(s.user_id) })}>{s.open}</Link> },
        { key: 'overdue', header: 'Overdue', cell: s => <Link className="link" to={metricLink('overdue', period, { owner_id: String(s.user_id) })}>{s.overdue}</Link> },
      ]} /></Card></>}</QueryView></div>
}
export function MetricPage() {
  const { metric = 'open' } = useParams()
  const [search] = useSearchParams()
  const q = useQuery({ queryKey: ['records', 'metric', metric, search.toString()], queryFn: () => api.get<{ items: RecordCase[] }>(`/api/staff/dashboard/metrics/${metric}?${search}`) })
  return <div className="space-y-6"><PageHeader title={LABELS[metric] ?? humanize(metric)} breadcrumbs={[{ label: 'Dashboard', to: `/staff/dashboard?${search}` }, { label: LABELS[metric] ?? metric }]} description="These are the requests included in this dashboard number." /><QueryView query={q}>{d => <CasesTable cases={d.items} />}</QueryView></div>
}
