import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Button, ButtonLink, Card, DateTime, ErrorAlert, Field, PageHeader, QueryView, TextInput, Textarea, Table, EmptyState } from '@/ui'
import type { RecordCase } from './types'
import { CasesTable } from './CasesTable'
import { useCommand } from './useCommand'
export function RecordsPage() {
  const [person, setPerson] = useState(''), [property, setProperty] = useState(''), [number, setNumber] = useState('')
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<number | null>(null), [reason, setReason] = useState('')
  const found = useQuery({ queryKey: ['records', 'search', search], queryFn: () => api.get<RecordCase[]>(`/api/records/search?${search}`) })
  const candidates = useQuery({ queryKey: ['records', 'disposal'], queryFn: () => api.get<(RecordCase & { retention_until: string })[]>('/api/records/disposal-candidates') })
  const holds = useQuery({ queryKey: ['records', 'holds'], queryFn: () => api.get<(RecordCase & { reason: string; placed_at: string })[]>('/api/records/legal-holds') })
  const dispose = useCommand(`/api/records/cases/${selected}/dispose`, 'Document files disposed of; history retained')
  const errors = isApiError(dispose.error) ? dispose.error.fields : {}
  return <div className="space-y-6"><PageHeader title="Finished requests and records" description="Find the history of finished requests, preserve records during a dispute, and review records eligible for disposal." />
    <Card title="Find finished requests"><form className="grid gap-4 sm:grid-cols-2" onSubmit={e => { e.preventDefault(); setSearch(new URLSearchParams({ person, property, number }).toString()) }}><Field label="Person"><TextInput value={person} onChange={e => setPerson(e.target.value)} /></Field><Field label="Property / Portion"><TextInput value={property} onChange={e => setProperty(e.target.value)} /></Field><Field label="Request number"><TextInput value={number} onChange={e => setNumber(e.target.value)} /></Field><Button type="submit">Search</Button></form></Card>
    <QueryView query={found}>{d => <CasesTable cases={d} />}</QueryView>
    <Card title="Disposal candidates" padded={false}><QueryView query={candidates}>{d => <Table caption="Records whose retention date has passed" rows={d} rowKey={r => r.id} empty={<EmptyState title="No records are ready for disposal" />} columns={[{ key: 'request', header: 'Request', cell: r => <ButtonLink variant="ghost" to={`/staff/cases/${r.id}`}>{r.number} — {r.title}</ButtonLink> }, { key: 'until', header: 'Retained until', cell: r => <DateTime value={r.retention_until} format="date" /> }, { key: 'action', header: 'Action', cell: r => <Button variant="danger-outline" onClick={() => { setSelected(r.id); setReason('') }}>Review disposal</Button> }]} />}</QueryView></Card>
    {selected != null && <Card title={`Dispose of request ${selected}`} description="This removes document files permanently. Metadata, case history, and decision text remain."><form className="space-y-4" onSubmit={e => { e.preventDefault(); dispose.mutate({ reason }, { onSuccess: () => setSelected(null) }) }}><ErrorAlert error={dispose.error} /><Field label="Reason for disposal" required error={errors.reason}><Textarea value={reason} onChange={e => setReason(e.target.value)} /></Field><Button type="submit" variant="danger" loading={dispose.isPending}>Confirm permanent disposal</Button> <Button variant="secondary" onClick={() => setSelected(null)}>Cancel</Button></form></Card>}
    <Card title="Active legal holds" padded={false}><QueryView query={holds}>{d => <Table caption="Records preserved from disposal" rows={d} rowKey={r => r.id} empty={<EmptyState title="No active legal holds" />} columns={[{ key: 'case', header: 'Request', cell: r => <ButtonLink variant="ghost" to={`/staff/cases/${r.id}`}>{r.number} — {r.title}</ButtonLink> }, { key: 'reason', header: 'Reason', cell: r => r.reason }, { key: 'at', header: 'Placed', cell: r => <DateTime value={r.placed_at} /> }]} />}</QueryView></Card>
  </div>
}
