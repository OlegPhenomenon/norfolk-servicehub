import { useState } from 'react'
import { Navigate, useParams, useSearchParams } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { useMe } from '@/auth/useMe'
import { panelsFor } from '@/registry'
import { Alert, Button, Card, DateTime, Dialog, EmptyState, ErrorAlert, Field, PageHeader, QueryView, Select, StatusPill, Steps, Table, Tabs, Textarea, TextInput, Timeline } from '@/ui'
import { FormRenderer, DocumentSlots } from '../services/FormRenderer'
import type { CaseDetail } from './types'
/** Applicant view. Staff are sent to the staff workspace for the same request. */
export function ResidentCasePage() {
  const { id } = useParams(); const me = useMe()
  if (me.data?.user?.kind === 'staff') return <Navigate to={`/staff/cases/${id}`} replace />
  return <CasePage staff={false} />
}
export function StaffCasePage() { return <CasePage staff /> }
function CasePage({ staff }: { staff: boolean }) {
  const { id } = useParams(); const [params, setParams] = useSearchParams()
  const q = useQuery({ queryKey: ['cases', 'detail', id], queryFn: () => api.get<CaseDetail>(`/api/cases/${id}`) })
  return <QueryView query={q}>{(d) => <><PageHeader eyebrow={d.case.number ?? 'Request'} title={d.case.service_name} description={staff ? `${d.case.applicant_name} · ${d.case.intake_channel} · Revision ${d.case.revision}` : d.applicant_status_text} meta={<StatusPill status={d.case.status} label={staff ? undefined : d.applicant_status_text} />} breadcrumbs={[{ label: staff ? 'Cases' : 'My requests', to: staff ? '/staff/cases' : '/my' }, { label: d.case.number ?? 'Request' }]} />{params.has('received') && <Alert tone="success" title="Your request has been received">Your reference is {d.case.number}. {d.case.module === 'venue_booking' ? 'Your booking is not confirmed yet.' : 'It is not approved yet.'}</Alert>}{d.required_action && <Card title={staff ? 'Waiting for applicant information' : 'Your reply is needed'}><p className="mb-4 whitespace-pre-wrap">{d.required_action.body}</p>{!staff && <><DocumentSlots caseId={d.case.id} definition={d.definition} answers={d.answers} /><Thread caseId={d.case.id} revision={d.case.revision} replyOnly /></>}{staff && <RecordReply detail={d} />}</Card>}<div className="my-6"><Steps steps={d.steps} current={d.case.current_step} finished={d.case.status === 'completed'} orientation="horizontal" /></div>{staff && d.guard_reason && <Alert tone="warning" title="This step needs an action">{d.guard_reason}</Alert>}<ActionBar detail={d} staff={staff} />{!staff && d.deadlines.map((deadline) => <p key={deadline.id} className="my-3 text-muted">{deadline.text}</p>)}<Tabs label="Request sections" value={params.get('tab') ?? 'overview'} onChange={(tab) => setParams((current) => { current.set('tab', tab); return current }, { replace: true })} tabs={[{ key: 'overview', label: 'Overview', content: <Card title="Submitted information"><FormRenderer definition={d.definition} answers={d.answers} onChange={() => undefined} review submitted decisionRefs={d.decision_refs} /></Card> }, { key: 'messages', label: 'Messages', content: <Thread caseId={d.case.id} staff={staff} revision={d.case.revision} canWrite={staff ? !!d.access.can_manage : ['submitted', 'in_progress', 'waiting_on_applicant'].includes(d.case.status)} /> }, ...(staff ? [{ key: 'notes', label: 'Internal notes', content: <Thread caseId={d.case.id} staff revision={d.case.revision} notes canWrite={!!d.access.can_manage} /> }, { key: 'assignments', label: 'Assignments', content: <Assignments detail={d} /> }, { key: 'deadlines', label: 'Deadlines', content: <Table caption="Request deadlines" rows={d.deadlines} rowKey={(r) => r.id} columns={[{ key: 'label', header: 'Target', cell: (r) => r.label }, { key: 'due', header: 'Due', cell: (r) => <DateTime value={r.due_at} /> }, { key: 'status', header: 'Status', cell: (r) => <StatusPill status={r.status} /> }, { key: 'pause', header: 'Pause days', cell: (r) => `${r.pause_days_used} of ${r.max_pause_days ?? 'not pausable'}` }]} /> }] : []), { key: 'representatives', label: 'Representatives', content: <Representatives caseId={d.case.id} staff={staff} /> }, { key: 'timeline', label: 'Timeline', content: <Timeline items={d.timeline.map((e) => ({ id: e.id, at: e.at, title: e.summary, actor: e.actor_name ?? undefined }))} /> }, ...panelsFor(d.case, staff ? 'staff' : 'applicant', d.definition).map((p) => ({ key: p.key, label: p.label, content: <p.Component caseId={d.case.id} /> }))]} /></>}</QueryView>
}
const LABELS: Record<string, string> = { advance: 'Complete this step', skip: 'Skip optional step', 'request-info': 'Request information', refuse: 'Refuse request', withdraw: 'Withdraw request', 'withdraw-on-behalf': "Withdraw on applicant's request", 'record-reply': "Record applicant's reply", cancel: 'Cancel request', reopen: 'Reopen request', 'close-duplicate': 'Close as duplicate', assign: 'Assign officer', escalate: 'Escalate to manager' }
const DESTRUCTIVE = ['cancel', 'refuse', 'withdraw', 'withdraw-on-behalf']
const ACTION_ORDER = ['advance', 'reopen', 'request-info', 'skip', 'escalate', 'close-duplicate', 'refuse', 'withdraw-on-behalf', 'withdraw', 'cancel']
/** Assisted requests: how the applicant contacted Council when staff act on their behalf. */
const CHANNELS = [{ value: 'phone', label: 'By phone' }, { value: 'post', label: 'By post' }, { value: 'email', label: 'By email' }, { value: 'walk_in', label: 'In person' }]
const ON_BEHALF = ['record-reply', 'withdraw-on-behalf']
function ActionBar({ detail: d, staff }: { detail: CaseDetail; staff: boolean }) {
  const [action, setAction] = useState('')
  // `record-reply` lives on the staff "Waiting for applicant information" card.
  const actions = d.allowed_actions.filter((a) => a !== 'assign' && a !== 'record-reply').sort((a, b) => ACTION_ORDER.indexOf(a) - ACTION_ORDER.indexOf(b))
  return <div className="my-5 flex flex-wrap gap-2">
    {actions.map((a) => <Button key={a} variant={DESTRUCTIVE.includes(a) ? 'danger-outline' : ['advance', 'reopen'].includes(a) ? 'primary' : 'secondary'} onClick={() => setAction(a)}>{LABELS[a] ?? a}</Button>)}
    <ActionDialog detail={d} staff={staff} action={action} onClose={() => setAction('')} />
  </div>
}
function RecordReply({ detail: d }: { detail: CaseDetail }) {
  const [open, setOpen] = useState(false)
  if (!d.allowed_actions.includes('record-reply')) return <p className="text-sm text-muted">{d.case.intake_channel === 'online' ? 'The applicant replies online. The request continues when they answer.' : 'The request continues when the applicant answers.'}</p>
  return <><p className="mb-3 text-sm text-muted">This is an assisted request. If the applicant answered Council by phone, post, email or in person, record their reply on their behalf.</p><Button variant="secondary" onClick={() => setOpen(true)}>{LABELS['record-reply']}</Button><ActionDialog detail={d} staff action={open ? 'record-reply' : ''} onClose={() => setOpen(false)} /></>
}
function ActionDialog({ detail: d, staff, action, onClose }: { detail: CaseDetail; staff: boolean; action: string; onClose: () => void }) {
  const client = useQueryClient()
  const [reason, setReason] = useState('')
  const [target, setTarget] = useState('')
  const [channel, setChannel] = useState('')
  const destructive = DESTRUCTIVE.includes(action)
  const onBehalf = ON_BEHALF.includes(action)
  const applicantWithdraw = !staff && action === 'withdraw'
  const close = () => { setReason(''); setTarget(''); setChannel(''); command.reset(); onClose() }
  const command = useMutation({ mutationFn: () => api.post(`/api/cases/${d.case.id}/${action === 'escalate' ? 'escalate' : `actions/${action}`}`, { expected_revision: d.case.revision, reason, body: ['request-info', 'record-reply'].includes(action) ? reason : undefined, of_case: action === 'close-duplicate' ? Number(target) : undefined, channel: onBehalf ? channel : undefined }), onSuccess: () => { close(); client.invalidateQueries({ queryKey: ['cases'] }) } })
  const errors = isApiError(command.error) ? command.error.fields : {}
  const title = applicantWithdraw ? 'Withdraw your request?' : destructive ? `${LABELS[action]}?` : LABELS[action] ?? 'Request action'
  const description = applicantWithdraw
    ? 'Council will stop working on your request and close it. Tell us why you are withdrawing it.'
    : action === 'withdraw-on-behalf'
      ? 'Use this only when the applicant asked Council to withdraw the request. The timeline records that you withdrew it on the applicant’s behalf.'
      : action === 'record-reply'
        ? 'Record what the applicant told Council. This answers the information request, resumes the request and its deadlines, and the timeline records that you recorded it on the applicant’s behalf.'
        : destructive ? 'This will close the request. Record a reason so the applicant and Council can understand the outcome.' : undefined
  const label = applicantWithdraw ? 'Why are you withdrawing this request?' : action === 'request-info' ? 'Information the applicant needs to provide' : action === 'record-reply' ? 'Applicant’s reply' : action === 'withdraw-on-behalf' ? 'Applicant’s reason for withdrawing' : destructive ? 'Reason' : 'Reason or completion note'
  const confirm = action === 'record-reply' ? 'Record reply' : destructive ? LABELS[action] : 'Confirm action'
  return <Dialog open={!!action} onClose={() => { if (!command.isPending) close() }} title={title} description={description} footer={<>
      <Button variant="secondary" onClick={close} disabled={command.isPending}>Back</Button>
      <Button variant={destructive ? 'danger' : 'primary'} disabled={action !== 'advance' && (!reason.trim() || (action === 'close-duplicate' && !target) || (onBehalf && !channel))} loading={command.isPending} onClick={() => command.mutate()}>{confirm}</Button>
    </>}>
      {command.error && (isApiError(command.error, 'conflict')
        ? <Alert tone="warning" title={action === 'advance' ? "This step can't be completed yet" : staff ? "This action can't be taken yet" : "Your request can't be withdrawn right now"}><p>{command.error.message}</p></Alert>
        : <ErrorAlert error={command.error} />)}
      {onBehalf && <Field label={action === 'record-reply' ? 'How the applicant replied' : 'How the applicant asked'} required error={errors.channel}><Select value={channel} onChange={(e) => setChannel(e.target.value)} placeholder="Choose how" options={CHANNELS} /></Field>}
      <Field label={label} required={action !== 'advance'} error={errors.reason || errors.body}><Textarea autoFocus value={reason} onChange={(e) => setReason(e.target.value)} /></Field>
      {action === 'close-duplicate' && <Field label="Original request ID" required error={errors.of_case}><TextInput type="number" min={1} value={target} onChange={(e) => setTarget(e.target.value)} /></Field>}
    </Dialog>
}
interface Message { id: number; body: string; created_at: string; author_name: string; from_staff?: boolean; requires_response?: boolean }
function Thread({ caseId, revision, staff = false, notes = false, replyOnly = false, canWrite = true }: { caseId: number; revision: number; staff?: boolean; notes?: boolean; replyOnly?: boolean; canWrite?: boolean }) {
  const client = useQueryClient(); const [body, setBody] = useState(''); const endpoint = `/api/cases/${caseId}/${notes ? 'notes' : 'messages'}`
  const q = useQuery({ queryKey: ['cases', notes ? 'notes' : 'messages', caseId], queryFn: () => api.get<{ items: Message[] }>(endpoint), enabled: !replyOnly })
  const post = useMutation({ mutationFn: () => api.post(endpoint, { body, expected_revision: staff ? revision : undefined }), onSuccess: () => { setBody(''); client.invalidateQueries({ queryKey: ['cases'] }) } })
  return <div className={notes ? 'rounded-lg bg-warning-50 p-5 space-y-4' : 'space-y-4'}>{notes && <Alert tone="warning" title="Internal — not visible to applicant">Use the Messages tab to contact the applicant.</Alert>}{!replyOnly && <QueryView query={q}>{({ items }) => items.length ? <div className="space-y-4">{items.map((m) => <Card key={m.id} title={m.author_name || (m.from_staff ? 'Council' : 'Applicant')}><p className="whitespace-pre-wrap break-words">{m.body}</p><p className="mt-2 text-sm text-muted"><DateTime value={m.created_at} /></p></Card>)}</div> : <EmptyState title={notes ? 'No internal notes' : 'No messages yet'} description={notes ? 'Record working notes here. Use Messages to contact the applicant.' : `Replies from Council and the applicant appear here.${canWrite ? ' Use the message field below to start a conversation.' : ''}`} />}</QueryView>}{canWrite && <form onSubmit={(e) => { e.preventDefault(); post.mutate() }}><Field label={notes ? 'Internal note' : 'Your message'} required error={isApiError(post.error) ? post.error.fields.body : undefined}><Textarea value={body} onChange={(e) => setBody(e.target.value)} maxLength={20000} /></Field>{post.error && <ErrorAlert error={post.error} />}<Button type="submit" className="mt-3" loading={post.isPending}>{notes ? 'Save internal note' : 'Send reply'}</Button><p aria-live="polite" className="mt-2 text-pine">{post.isSuccess ? 'Saved.' : ''}</p></form>}</div>
}
function Assignments({ detail: d }: { detail: CaseDetail }) {
  const client = useQueryClient(); const [user, setUser] = useState(''); const [reason, setReason] = useState(''); const [role, setRole] = useState('owner')
  const users = useQuery({ queryKey: ['cases', 'staff-users'], queryFn: () => api.get<{ items: { id: number; name: string }[] }>('/api/staff/users') })
  const assign = useMutation({ mutationFn: () => api.post(`/api/cases/${d.case.id}/assign`, { user_id: Number(user), role, reason, replace_owner: role === 'owner', expected_revision: d.case.revision }), onSuccess: () => client.invalidateQueries({ queryKey: ['cases'] }) })
  const end = useMutation({ mutationFn: (id: number) => api.post(`/api/cases/${d.case.id}/assignments/${id}/end`, { reason, expected_revision: d.case.revision }), onSuccess: () => client.invalidateQueries({ queryKey: ['cases'] }) })
  return <div className="space-y-5">{d.allowed_actions.includes('assign') && <Card title="Assign or replace an officer"><div className="grid gap-4 sm:grid-cols-2"><Field label="Officer" required><Select value={user} onChange={(e) => setUser(e.target.value)} placeholder="Choose an officer" options={users.data?.items.map((u) => ({ value: String(u.id), label: u.name })) ?? []} /></Field><Field label="Assignment role" required><Select value={role} onChange={(e) => setRole(e.target.value)} options={[{ value: 'owner', label: 'Owner — replaces current owner' }, { value: 'collaborator', label: 'Collaborator' }]} /></Field></div><Field label="Reason for assigning or ending an assignment" required><Textarea value={reason} onChange={(e) => setReason(e.target.value)} /></Field><Button className="mt-3" onClick={() => assign.mutate()} loading={assign.isPending}>Assign officer</Button>{assign.error && <ErrorAlert error={assign.error} />}{end.error && <ErrorAlert error={end.error} />}</Card>}<Table caption="Assignment history" rows={d.assignments ?? []} rowKey={(a) => a.id} columns={[{ key: 'name', header: 'Officer', cell: (a) => `${a.name} (${a.role})` }, { key: 'why', header: 'Reason', cell: (a) => a.reason }, { key: 'by', header: 'Assigned by', cell: (a) => a.assigned_by }, { key: 'when', header: 'Assigned', cell: (a) => <DateTime value={a.assigned_at} /> }, { key: 'end', header: 'Ended', cell: (a) => a.ended_at ? <><DateTime value={a.ended_at} /> {a.ended_reason}</> : d.allowed_actions.includes('assign') ? <Button variant="ghost" onClick={() => end.mutate(a.id)} loading={end.isPending}>End assignment</Button> : 'Active' }]} empty={<EmptyState title="No officer assignments" />} /></div>
}
function Representatives({ caseId, staff }: { caseId: number; staff: boolean }) {
  const client = useQueryClient(); const [email, setEmail] = useState(''); const [basis, setBasis] = useState('')
  const q = useQuery({ queryKey: ['cases', 'representatives', caseId], queryFn: () => api.get<{ items: { id: number; name: string; basis: string; status: string }[]; can_manage_representatives: boolean }>(`/api/cases/${caseId}/representatives`) })
  const change = useMutation({ mutationFn: (rid?: number) => rid ? api.post(`/api/cases/${caseId}/representatives/${rid}/revoke`) : api.post(`/api/cases/${caseId}/representatives`, { email, basis }), onSuccess: () => client.invalidateQueries({ queryKey: ['cases'] }) })
  return <Card title="Authorised representatives"><QueryView query={q}>{({ items, can_manage_representatives: manage }) => <>
    <p className="mb-4">{staff ? 'People the applicant authorised to act on this request. Only the applicant can add or revoke them.' : manage ? 'Authorise an existing resident account. Revoking access takes effect immediately.' : 'You act on this request as an authorised representative. Only the applicant can add or revoke representatives.'}</p>
    {items.length ? <div className="space-y-3">{items.map((r) => <p key={r.id}>{r.name}: {r.basis} — {r.status} {manage && r.status === 'active' && <Button variant="ghost" onClick={() => change.mutate(r.id)}>Revoke</Button>}</p>)}</div> : <EmptyState title="No representatives" />}
    {manage && <div className="mt-4 space-y-3"><Field label="Representative's email" required><TextInput type="email" value={email} onChange={(e) => setEmail(e.target.value)} /></Field><Field label="Authorisation basis" required><Textarea value={basis} onChange={(e) => setBasis(e.target.value)} /></Field><Button onClick={() => change.mutate(undefined)} loading={change.isPending}>Add representative</Button></div>}
    {change.error && <ErrorAlert error={change.error} />}
  </>}</QueryView></Card>
}
