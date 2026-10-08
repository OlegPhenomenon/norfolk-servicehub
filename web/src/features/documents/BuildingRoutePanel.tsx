import { useState } from 'react'
import { Link, useLocation } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Alert, Badge, Button, Card, Checkbox, DateTime, EmptyState, ErrorAlert, Field, Money, QueryView, RadioGroup, StatusPill, TextInput, Textarea, useToast } from '@/ui'
import { cents } from '@/features/finance/forms'
import type { BuildingRoute, FeeAssessment, FeeView } from './types'
import { APPROVALS, MODIFICATION_TYPES, approvalLabel } from './labels'

const RULES: Record<string, string> = { building_works_scale: 'Building Development and Works scale', standard_modification: 'Standard modification (Building and Works scale)', basic_modification: 'Basic modification (lapse date only)', staff_assessment: 'Staff assessment' }

function useAction(url: string, onDone?: () => void) {
  const qc = useQueryClient(), toast = useToast()
  return useMutation({
    mutationFn: (body: () => Record<string, unknown>) => api.post(url, body()),
    onSuccess: async () => { toast.success('Saved.'); await Promise.all([qc.invalidateQueries({ queryKey: ['documents'] }), qc.invalidateQueries({ queryKey: ['cases'] }), qc.invalidateQueries({ queryKey: ['finance'] })]); onDone?.() },
  })
}

/** Fee assessment (N-01), approval scope (N-02) and public exhibition state (N-03) of a building request. */
export function BuildingRoutePanel({ caseId }: { caseId: number }) {
  // Keyed under 'cases' so workflow actions (which refresh case queries) also refresh the revision used here.
  const q = useQuery({ queryKey: ['cases', 'route', caseId], queryFn: () => api.get<BuildingRoute>(`/api/cases/${caseId}/building-route`) })
  return <QueryView query={q}>{r => !r.route && !r.fee.applies && !r.exhibition_step
    ? <EmptyState title="No approval route steps" description="This request has no fee assessment, approval scope or public exhibition stage." />
    : <div className="space-y-5 break-words">
      {r.fee.applies && <FeeCard caseId={caseId} fee={r.fee} revision={r.revision} />}
      {r.route && <ScopeCard caseId={caseId} data={r} />}
      {r.exhibition && <ExhibitionCard caseId={caseId} data={r} />}
    </div>}</QueryView>
}

function AssessmentItem({ a }: { a: FeeAssessment }) {
  return <li className="border-b border-line py-3">
    <p className="font-semibold">Assessment v{a.version} · {RULES[a.rule] ?? a.rule} · <Money cents={a.amount_cents} /></p>
    <p className="text-sm text-muted">{a.method === 'manual' ? 'Set by staff' : 'Schedule calculation'}{a.assessed_by ? ` · ${a.assessed_by}` : ''} · <DateTime value={a.assessed_at} /></p>
    <p className="mt-2">{a.explanation}</p>
    {a.reason && <p className="mt-2">Basis / reason: {a.reason}</p>}
    {a.inputs.estimated_cost_cents !== null && <p className="text-sm">Estimated cost used: <Money cents={a.inputs.estimated_cost_cents} /> ({a.inputs.estimated_cost_source === 'staff' ? 'recorded by staff' : 'from the application'})</p>}
    {a.inputs.modification_types && <p className="text-sm">Modification types used: {a.inputs.modification_types.map(t => MODIFICATION_TYPES.find(m => m.value === t)?.label ?? t).join(', ')}</p>}
    {a.adjustment && <p className="mt-2">{a.adjustment.kind === 'credit_note' ? 'Credit note' : 'Supplementary invoice'} {a.adjustment.number} for <Money cents={a.adjustment.total_cents} />. Earlier invoices are unchanged.</p>}
  </li>
}

function FeeCard({ caseId, fee, revision }: { caseId: number; fee: FeeView; revision: number }) {
  const assessments = fee.assessments ?? []
  const [method, setMethod] = useState('schedule')
  const [cost, setCost] = useState(fee.application?.estimated_cost_cents != null ? (fee.application.estimated_cost_cents / 100).toFixed(2) : '')
  const [types, setTypes] = useState<string[]>(fee.application?.modification_types ?? [])
  const [amount, setAmount] = useState(''), [reason, setReason] = useState('')
  const m = useAction(`/api/cases/${caseId}/building-fee`, () => setReason(''))
  const errors = isApiError(m.error) ? m.error.fields : {}
  return <Card title="Fee assessment" description={fee.schedule_note}>
    {fee.proposal ? <Alert title={`System calculation: ${RULES[fee.proposal.rule] ?? fee.proposal.rule}`}><p><Money cents={fee.proposal.amount_cents} /> — {fee.proposal.explanation}</p><p className="mt-2 text-sm">Based on the application answers. Staff confirm or replace it below.</p></Alert>
      : <Alert tone="warning" title="No schedule calculation">{fee.proposal_error}</Alert>}
    <h3 className="mt-5 font-semibold">Recorded assessments</h3>
    {assessments.length ? <ol aria-label="Fee assessment history">{assessments.map(a => <AssessmentItem key={a.id} a={a} />)}</ol> : <p className="text-muted">No fee has been assessed yet. The request cannot be invoiced or decided until it is.</p>}
    <p className="mt-3">{fee.invoiced ? (fee.settled ? 'Invoice issued and settled.' : 'Invoice issued — payment outstanding. Decisions cannot be issued until it is paid.') : 'Not invoiced yet: the invoice is issued when the request enters the payment step.'}</p>
    {(fee.waivers ?? []).map(w => <Alert key={w.item_code} tone="info" title={`Approved exemption: ${w.item_code}`}><Money cents={w.amount_cents} /> waived — {w.reason}. Approved by {w.approved_by}.</Alert>)}
    {fee.can_assess && <form className="mt-5 space-y-4" onSubmit={e => { e.preventDefault(); m.mutate(() => ({ method, estimated_cost: cost.trim(), ...(fee.route === 'modification' ? { modification_types: types } : {}), ...(method === 'manual' ? { amount_cents: cents(amount) } : {}), reason, expected_revision: revision })) }} noValidate>
      <RadioGroup legend="Assessment method" name={`fee-method-${caseId}`} value={method} onChange={setMethod} inline options={[{ value: 'schedule', label: 'Apply the fee schedule' }, { value: 'manual', label: 'Staff assessment (enter amount and basis)' }]} error={errors.method} />
      <Field label="Total estimated cost of building and works (AUD)" hint="Change it only with a reason; the application value is kept on record." error={errors.estimated_cost}><TextInput inputMode="decimal" value={cost} onChange={e => setCost(e.target.value)} /></Field>
      {fee.route === 'modification' && <fieldset className="space-y-2"><legend className="font-semibold">Types of modification used for the fee</legend>{MODIFICATION_TYPES.map(t => <Checkbox key={t.value} label={t.label} checked={types.includes(t.value)} onChange={e => setTypes(e.target.checked ? [...types, t.value] : types.filter(v => v !== t.value))} />)}{errors.modification_types && <p role="alert" className="text-danger">{errors.modification_types}</p>}</fieldset>}
      {method === 'manual' && <Field label="Assessed fee (AUD)" required error={errors.amount_cents}><TextInput inputMode="decimal" value={amount} onChange={e => setAmount(e.target.value)} /></Field>}
      <Field label="Basis / reason" required={method === 'manual' || assessments.length > 0} hint="Required for a staff assessment, a re-assessment or changed inputs." error={errors.reason}><Textarea value={reason} onChange={e => setReason(e.target.value)} /></Field>
      {m.error && <ErrorAlert error={m.error} />}
      <Button type="submit" loading={m.isPending}>{assessments.length ? 'Record re-assessment' : 'Record fee assessment'}</Button>
      <p className="text-sm text-muted">A re-assessment after invoicing never edits the issued invoice: an increase is billed on a supplementary invoice and a decrease is returned by a credit note. Fee exemptions are approved by a manager in the Money tab before the invoice is issued.</p>
    </form>}
  </Card>
}

function ScopeCard({ caseId, data }: { caseId: number; data: BuildingRoute }) {
  const modification = data.route === 'modification'
  const current = data.scope
  const [approvals, setApprovals] = useState<string[]>(current?.approvals ?? [])
  const [originals, setOriginals] = useState<number[]>(current?.originals ?? data.originals.map(o => o.decision_id))
  const [reason, setReason] = useState('')
  const m = useAction(`/api/cases/${caseId}/approval-scope`, () => setReason(''))
  const errors = isApiError(m.error) ? m.error.fields : {}
  const describe = (s: { approvals?: string[]; originals?: number[] }) => s.approvals ? s.approvals.map(approvalLabel).join(' and ') : (s.originals ?? []).map(id => `${approvalLabel(data.originals.find(o => o.decision_id === id)?.approval_type ?? '')} #${id}`).join(' and ')
  return <Card title={modification ? 'Approvals being modified' : 'Approvals in scope'} description="Council staff confirm which approvals this request covers. Only those decisions are required.">
    {current ? <p className="flex flex-wrap items-center gap-2"><span className="font-semibold">{describe(current)}</span>{current.confirmed ? <Badge tone="success">Confirmed by Council</Badge> : <Badge tone="warning">Requested — awaiting staff confirmation</Badge>}</p> : <p className="text-muted">This request was lodged before approval scopes were recorded; its workflow requires every listed decision.</p>}
    {modification && <ul className="mt-3 space-y-1" aria-label="Original approvals">{data.originals.map(o => <li key={o.decision_id}>{approvalLabel(o.approval_type)} #{o.decision_id} ({o.case_number}){o.in_scope ? '' : ' — not in scope'}{o.modification_decision_id ? ` — modification decision #${o.modification_decision_id}` : ''}</li>)}</ul>}
    {data.scope_history.length > 0 && <details className="mt-3"><summary className="cursor-pointer py-2 text-primary">Scope history</summary><ol>{data.scope_history.map((h, i) => <li key={i} className="border-b border-line py-2"><p>{describe(h.scope)} — {h.source === 'staff' ? `confirmed by ${h.set_by ?? 'staff'}` : 'requested by the applicant'} · <DateTime value={h.set_at} /></p><p className="text-sm">Reason: {h.reason}</p></li>)}</ol></details>}
    {data.can_scope && <form className="mt-4 space-y-3" onSubmit={e => { e.preventDefault(); m.mutate(() => ({ ...(modification ? { originals } : { approvals }), reason, expected_revision: data.revision })) }} noValidate>
      <fieldset className="space-y-2"><legend className="font-semibold">{modification ? 'Original approvals in scope' : 'Approvals this request needs'}</legend>
        {modification ? data.originals.map(o => <Checkbox key={o.decision_id} label={`${approvalLabel(o.approval_type)} #${o.decision_id} (${o.case_number ?? ''})`} checked={originals.includes(o.decision_id)} onChange={e => setOriginals(e.target.checked ? [...originals, o.decision_id] : originals.filter(v => v !== o.decision_id))} />)
          : APPROVALS.map(a => <Checkbox key={a.value} label={a.label} checked={approvals.includes(a.value)} onChange={e => setApprovals(e.target.checked ? [...approvals, a.value] : approvals.filter(v => v !== a.value))} />)}
        {(errors.approvals || errors.originals) && <p role="alert" className="text-danger">{errors.approvals ?? errors.originals}</p>}
      </fieldset>
      <Field label="Reason" required error={errors.reason}><Textarea value={reason} onChange={e => setReason(e.target.value)} /></Field>
      {m.error && <ErrorAlert error={m.error} />}
      <Button type="submit" loading={m.isPending}>Confirm approval scope</Button>
    </form>}
  </Card>
}

function ExhibitionCard({ caseId, data }: { caseId: number; data: BuildingRoute }) {
  const e = data.exhibition!, staff = useLocation().pathname.startsWith('/staff')
  const [reason, setReason] = useState('')
  const m = useAction(`/api/cases/${caseId}/exhibition-not-required`, () => setReason(''))
  const errors = isApiError(m.error) ? m.error.fields : {}
  return <Card title="Public exhibition" description="Either the proposal is exhibited and every public submission is considered, or staff record that exhibition is not required, with the reason.">
    {e.block && <Alert tone="warning" title="Exhibition stage not finished">{e.block}</Alert>}
    {e.not_required && <Alert tone="info" title="Exhibition not required for this request">{e.not_required.reason} — recorded{e.not_required.decided_by ? ` by ${e.not_required.decided_by}` : ''} <DateTime value={e.not_required.decided_at} />.</Alert>}
    {e.exhibitions.length > 0 && <ul className="mt-3 space-y-3" aria-label="Exhibitions">{e.exhibitions.map(x => <li key={x.id} className="border-b border-line pb-3">
      <p className="flex flex-wrap items-center gap-2 font-semibold">{x.title} <StatusPill status={x.status} /></p>
      {x.closes_at && <p>Comments close <DateTime value={x.closes_at} /></p>}
      {x.termination_reason && <p>Terminated early: {x.termination_reason}</p>}
      {x.withdrawal_reason && <p>Withdrawn by a manager: {x.withdrawal_reason}{x.withdrawn_at && <> (<DateTime value={x.withdrawn_at} />)</>}</p>}
      <p>{x.submissions} public submission(s), {x.pending_submissions} awaiting a consideration outcome.</p>
      {x.consideration_summary && <p>Consideration summary: {x.consideration_summary}</p>}
      {staff && <Link className="link inline-block py-2" to={`/staff/exhibitions/${x.id}`}>Open exhibition and submissions</Link>}
    </li>)}</ul>}
    {data.can_exhibit && staff && <div className="mt-4 space-y-3">
      <Link className="link inline-block" to="/staff/exhibitions">Prepare a public exhibition</Link>
      {!e.exhibitions.length && !e.not_required && <form className="space-y-3" onSubmit={ev => { ev.preventDefault(); m.mutate(() => ({ reason, expected_revision: data.revision })) }} noValidate>
        <Field label="Reason exhibition is not required" required error={errors.reason}><Textarea value={reason} onChange={ev => setReason(ev.target.value)} /></Field>
        {m.error && <ErrorAlert error={m.error} />}
        <Button type="submit" variant="secondary" loading={m.isPending}>Record exhibition not required</Button>
      </form>}
    </div>}
  </Card>
}
