import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { useMe, hasRole } from '@/auth/useMe'
import { Alert, Button, Card, Checkbox, DateTime, DescriptionList, ErrorAlert, Field, QueryView, Textarea, buttonClasses } from '@/ui'
import type { ComplaintPanelData, Delivery, RecordPanel } from './types'
import { DeliveryTable } from './IntegrationsPage'
import { useCommand } from './useCommand'
export function ComplaintPanel({ caseId }: { caseId: number }) {
  const q = useQuery({ queryKey: ['records', 'complaint', caseId], queryFn: () => api.get<ComplaintPanelData>(`/api/cases/${caseId}/complaint`) })
  return <QueryView query={q}>{d => <ComplaintContents key={`${caseId}-${d.case.revision}`} caseId={caseId} data={d} />}</QueryView>
}
function ComplaintContents({ caseId, data }: { caseId: number; data: ComplaintPanelData }) {
  const [selected, setSelected] = useState<number[]>(data.subjects ?? [])
  const [reason, setReason] = useState('')
  const navigate = useNavigate()
  const save = useCommand(`/api/cases/${caseId}/complaint/subjects`, 'Complaint subjects recorded')
  const review = useCommand(`/api/cases/${caseId}/complaint/request-review`, 'Review requested')
  const fields = isApiError(review.error) ? review.error.fields : {}
  return <div className="space-y-5">
    <Alert tone="warning" title="Confidential feedback">{data.hidden_from ? `Confidential — hidden from: ${data.hidden_from.map(p => p.display_name).join(', ') || 'no explicit exclusions yet'}. Only the applicant, authorised representatives, complaints officers and managers with case access can view it.` : 'Your feedback is handled confidentially by an authorised officer.'}</Alert>
    {data.can_triage && <Card title="Staff who are subjects of this complaint" description="Existing exclusions remain in place. Exclusions override every staff role."><form className="space-y-4" onSubmit={e => { e.preventDefault(); save.mutate({ staff_user_ids: selected, expected_revision: data.case.revision }) }}><ErrorAlert error={save.error} /><fieldset className="space-y-2"><legend className="mb-2 font-semibold">Select staff members</legend>{data.staff?.map(p => <Checkbox key={p.id} label={p.display_name} checked={selected.includes(p.id)} disabled={data.subjects?.includes(p.id)} onChange={e => setSelected(e.target.checked ? [...selected, p.id] : selected.filter(id => id !== p.id))} />)}</fieldset><Button type="submit" loading={save.isPending}>Record subjects and restrict access</Button></form></Card>}
    {data.links.length > 0 && <Card title="Related reviews"><ul className="space-y-2">{data.links.map(l => <li key={l.id}><Link className="link" to={`${data.hidden_from ? '/staff' : '/my'}/cases/${l.id}`}>{l.number} — {l.title}</Link></li>)}</ul></Card>}
    {data.can_request_review && <Card title="Ask for a review" description="A new confidential request will be linked to this feedback and assigned to a different handler. Both histories stay available."><form className="space-y-4" onSubmit={e => { e.preventDefault(); review.mutate({ reason }, { onSuccess: result => { const r = result as { id: number }; void navigate(`/my/cases/${r.id}`) } }) }}><ErrorAlert error={review.error} /><Field label="Why would you like a review?" required error={fields.reason}><Textarea value={reason} onChange={e => setReason(e.target.value)} maxLength={5000} /></Field><Button type="submit" loading={review.isPending}>Ask for a review</Button></form></Card>}
  </div>
}
export function RecordsPanel({ caseId }: { caseId: number }) {
  const { data: me } = useMe()
  const q = useQuery({ queryKey: ['records', 'panel', caseId], queryFn: () => api.get<RecordPanel>(`/api/records/cases/${caseId}`) })
  const deliveries = useQuery({ queryKey: ['records', 'case-deliveries', caseId], queryFn: () => api.get<Delivery[]>(`/api/cases/${caseId}/integrations`) })
  const [reason, setReason] = useState('')
  const hold = useCommand(`/api/records/cases/${caseId}/legal-hold${q.data?.legal_hold ? '/release' : ''}`, 'Legal hold updated')
  const fields = isApiError(hold.error) ? hold.error.fields : {}
  return <div className="space-y-5"><Card title="Records" actions={<a className={buttonClasses('secondary')} href={`/api/cases/${caseId}/export.zip`}>Export case ZIP</a>}><QueryView query={q}>{d => <><DescriptionList items={[{ label: 'Retention date', value: d.retention_until ? <DateTime value={d.retention_until} format="date" /> : 'Set when this request closes' }, { label: 'Legal hold', value: d.legal_hold ? 'Documents preserved — disposal blocked' : 'No active hold' }]} />{d.holds.filter(h => !h.released_at).map(h => <Alert key={h.placed_at} tone="warning" title="Active legal hold">{h.reason}</Alert>)}{d.disposed.length > 0 && <Alert title="Document files disposed of">Case metadata, events and decision text remain available.</Alert>}{hasRole(me, 'manager') && <form className="mt-5 space-y-4" onSubmit={e => { e.preventDefault(); hold.mutate({ reason, expected_revision: q.data?.revision }, { onSuccess: () => setReason('') }) }}><ErrorAlert error={hold.error} /><Field label={d.legal_hold ? 'Reason to release the hold' : 'Reason for preservation'} required error={fields.reason}><Textarea value={reason} onChange={e => setReason(e.target.value)} /></Field><Button type="submit" loading={hold.isPending}>{d.legal_hold ? 'Release legal hold' : 'Place legal hold'}</Button></form>}</>}</QueryView></Card><Card title="External records"><QueryView query={deliveries}>{d => <DeliveryTable deliveries={d} />}</QueryView></Card></div>
}
