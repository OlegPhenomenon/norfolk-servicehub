import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Alert, Button, ButtonLink, Card, Checkbox, DateTime, EmptyState, ErrorAlert, Field, PageHeader, QueryView, Select, StatusPill, Table } from '@/ui'
import type { Delivery, ExternalSystem } from './types'
import { useCommand } from './useCommand'
export function DeliveryTable({ deliveries, admin = false }: { deliveries: Delivery[]; admin?: boolean }) {
  return <><Table caption="External records deliveries" rows={deliveries} rowKey={r => r.id} empty={<EmptyState title="No deliveries yet" description="Closed requests and issued decisions are sent to Content Manager. Confirmed receipts are sent to Civica Altitude." />} columns={[
    { key: 'system', header: 'System / operation', cell: r => <><span className="block">{r.system_code === 'content_manager' ? 'Content Manager' : 'Civica Altitude'}</span><span className="text-sm text-muted">{r.kind.replaceAll('.', ' ').replaceAll('_', ' ')}</span>{r.case_id && <ButtonLink variant="ghost" to={`/staff/cases/${r.case_id}`}>Open request</ButtonLink>}</> },
    { key: 'status', header: 'Status', cell: r => <StatusPill status={r.status} /> },
    { key: 'reference', header: 'External reference', cell: r => r.external_ref ?? 'Awaiting acceptance' },
    { key: 'error', header: 'Last error', cell: r => r.last_error ?? '—' },
    { key: 'updated', header: 'Updated', cell: r => <DateTime value={r.updated_at} /> },
    ...(admin ? [{ key: 'retry', header: 'Action', cell: (r: Delivery) => ['failed', 'dead'].includes(r.status) ? <RetryDelivery id={r.id} /> : '—' }] : []),
  ]} /></>
}
function RetryDelivery({ id }: { id: number }) {
  const retry = useCommand(`/api/admin/integrations/${id}/retry`, 'Delivery queued for retry')
  return <><Button variant="secondary" loading={retry.isPending} onClick={() => retry.mutate({})}>Retry</Button><ErrorAlert error={retry.error} /></>
}
function SystemControls({ system }: { system: ExternalSystem }) {
  const update = useCommand(`/api/admin/integrations/systems/${system.code}`, 'Mock system updated')
  const [showRemote, setShowRemote] = useState(false)
  const remote = useQuery({ queryKey: ['records', 'remote', system.code], queryFn: () => api.get<{ id: number; operation_id: string; external_ref: string; received_at: string; payload_json: string | null }[]>(`/api/admin/mock-records/${system.code}`), enabled: showRemote })
  return <Card title={system.name}><ErrorAlert error={update.error} /><div className="flex flex-wrap gap-5"><Checkbox label="Simulate outage" checked={system.outage === 1} disabled={update.isPending} onChange={e => update.mutate({ outage: e.target.checked, drop_responses: system.drop_responses === 1 })} /><Checkbox label="Accept record but lose the first response" checked={system.drop_responses === 1} disabled={update.isPending} onChange={e => update.mutate({ outage: system.outage === 1, drop_responses: e.target.checked })} /><Button variant="secondary" onClick={() => setShowRemote(!showRemote)}>{showRemote ? 'Hide' : 'View'} remote system contents</Button></div>
    {showRemote && <QueryView query={remote}>{d => <Table caption="Records held by the mock remote system" rows={d} rowKey={r => r.id} empty={<EmptyState title="The remote system has no records" />} columns={[{ key: 'operation', header: 'Operation ID', cell: r => r.operation_id }, { key: 'reference', header: 'Reference', cell: r => r.external_ref }, { key: 'at', header: 'Received', cell: r => <DateTime value={r.received_at} /> }, { key: 'payload', header: 'Content', cell: r => r.payload_json ? <details><summary>View accessible content</summary><pre className="max-w-md whitespace-pre-wrap break-all text-xs">{r.payload_json}</pre></details> : 'Requires case access' }]} />}</QueryView>}
  </Card>
}
export function IntegrationsPage() {
  const [status, setStatus] = useState(''), [system, setSystem] = useState('')
  const filters = new URLSearchParams(); if (status) filters.set('status', status); if (system) filters.set('system', system)
  const q = useQuery({ queryKey: ['records', 'integrations', status, system], queryFn: () => api.get<Delivery[]>(`/api/admin/integrations?${filters}`) })
  const systems = useQuery({ queryKey: ['records', 'systems'], queryFn: () => api.get<ExternalSystem[]>('/api/admin/integrations/systems') })
  return <div className="space-y-6"><PageHeader title="Integrations" description="Monitor delivery and retry failed records without creating duplicates." /><Alert title="Mock systems — test mode">These receivers demonstrate the exchange cycle; connections to Council's production systems have not been verified.</Alert>
    <QueryView query={systems}>{d => <div className="grid gap-4 lg:grid-cols-2">{d.map(s => <SystemControls key={s.code} system={s} />)}</div>}</QueryView>
    <Card title="Delivery log"><div className="grid gap-4 sm:grid-cols-2"><Field label="Status"><Select value={status} onChange={e => setStatus(e.target.value)} options={[{ value: '', label: 'All statuses' }, ...['pending', 'sending', 'accepted', 'failed', 'dead'].map(value => ({ value, label: value === 'dead' ? 'Stopped after retries' : value.charAt(0).toUpperCase() + value.slice(1) }))]} /></Field><Field label="System"><Select value={system} onChange={e => setSystem(e.target.value)} options={[{ value: '', label: 'All systems' }, { value: 'content_manager', label: 'Content Manager' }, { value: 'civica_altitude', label: 'Civica Altitude' }]} /></Field></div><QueryView query={q}>{d => <DeliveryTable deliveries={d} admin />}</QueryView></Card>
  </div>
}
