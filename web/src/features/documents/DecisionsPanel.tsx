import { useState } from 'react'
import { Link, useLocation } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Card, QueryView, EmptyState, DateTime, Badge, StatusPill, Alert, Button, Field, Textarea, Select, Checkbox, ErrorAlert, useToast } from '@/ui'
import type { Decision, DecisionList, Document, Template } from './types'

export function DecisionsPanel({ caseId }: { caseId: number }) {
  const area = useLocation().pathname.startsWith('/staff') ? 'staff' : 'my'
  const q = useQuery({ queryKey: ['documents', 'decisions', caseId], queryFn: () => api.get<DecisionList>(`/api/cases/${caseId}/decisions`) })
  return <QueryView query={q}>{data => <div className="space-y-5 break-words">{data.building_project_id && <Link className="link inline-block py-3" to={`/${area}/projects/${data.building_project_id}`}>View building project and approval history</Link>}{data.can_prepare && <><Alert title={data.authorities.length ? `You hold authority for: ${data.authorities.map(t => t.replaceAll('_', ' ')).join(', ')}` : 'Needs approval by someone with authority'}>Each development and building approval is a separate decision, with its own evidence versions.</Alert><details><summary className="cursor-pointer py-3 text-primary">Prepare a decision</summary><DecisionEditor caseId={caseId} revision={data.revision} /></details></>}
    {!data.items.length && <EmptyState title="No decisions yet" description="Issued results will be available here to download." />}
    {data.items.map(d => <Card key={d.id} title={d.decision_type.replaceAll('_', ' ')} actions={<StatusPill status={d.status} />}><p className="font-semibold">{d.outcome.replaceAll('_', ' ')}</p><p className="break-words whitespace-pre-wrap mt-3">{d.reasons}</p>{d.conditions && <p className="break-words whitespace-pre-wrap mt-3">Conditions: {d.conditions}</p>}{d.issued_at && <DateTime value={d.issued_at} />}
      <div className="flex flex-wrap gap-2 mt-4" aria-label="Decision based on">{d.evidence.map(e => <Badge key={e.id}>Decision based on {e.title} v{e.version}</Badge>)}</div>
      {d.output_document_version_id && <a className="link inline-block py-3" href={`/api/document-versions/${d.output_document_version_id}/download`}>Download issued decision</a>}
      {d.returned_reason && <Alert tone="warning" title="Changes requested">{d.returned_reason}</Alert>}
      {data.can_prepare && ['draft', 'returned'].includes(d.status) && <details><summary className="cursor-pointer py-3 text-primary">Edit draft and evidence</summary><DecisionEditor key={d.id} caseId={caseId} revision={data.revision} decision={d} /></details>}
      {data.staff && <DecisionActions caseId={caseId} decision={d} revision={data.revision} authority={data.authorities.includes(d.decision_type)} canPrepare={data.can_prepare} />}
    </Card>)}
  </div>}</QueryView>
}
function DecisionEditor({ caseId, revision, decision }: { caseId: number; revision: number; decision?: Decision }) {
  const [type, setType] = useState(decision?.decision_type ?? ''), [templateId, setTemplate] = useState(String(decision?.template_id ?? '')), [outcome, setOutcome] = useState(decision?.outcome ?? 'approved'), [reasons, setReasons] = useState(decision?.reasons ?? ''), [conditions, setConditions] = useState(decision?.conditions ?? ''), [selected, setSelected] = useState<number[] | null>(decision?.evidence.map(e => e.id) ?? null)
  const templates = useQuery({ queryKey: ['documents', 'templates'], queryFn: () => api.get<Template[]>('/api/decision-templates') })
  const docs = useQuery({ queryKey: ['documents', 'case', caseId], queryFn: () => api.get<Document[]>(`/api/cases/${caseId}/documents`) })
  const latest = (docs.data ?? []).filter(d => d.visibility === 'applicant' && !['decision', 'letter', 'certificate'].includes(d.category)).flatMap(d => d.versions.at(-1)?.id ?? [])
  const evidence = selected ?? latest, qc = useQueryClient(), toast = useToast()
  const mutation = useMutation({ mutationFn: () => { const input = { decision_type: type, template_id: Number(templateId), outcome, reasons, conditions, evidence_version_ids: evidence, expected_revision: revision }; return decision ? api.put(`/api/cases/${caseId}/decisions/${decision.id}`, input) : api.post(`/api/cases/${caseId}/decisions`, input) }, onSuccess: async () => { toast.success('Draft decision saved.'); await qc.invalidateQueries({ queryKey: ['documents'] }); await qc.invalidateQueries({ queryKey: ['cases'] }) } })
  const errors = isApiError(mutation.error) ? mutation.error.fields : {}
  const types = [...new Set((templates.data ?? []).filter(t => !t.decision_type.endsWith('_response')).map(t => t.decision_type))]
  return <form className="space-y-4 mt-4" onSubmit={e => { e.preventDefault(); mutation.mutate() }}>
    {templates.error && <ErrorAlert error={templates.error} />}<Field label="Decision type" required error={errors.decision_type}><Select disabled={!!decision} placeholder="Choose an approval" value={type} onChange={e => { setType(e.target.value); setTemplate('') }} options={types.map(t => ({ value: t, label: t.replaceAll('_', ' ') }))} /></Field>
    <Field label="Versioned template" required error={errors.template_id}><Select placeholder="Choose a template" value={templateId} onChange={e => setTemplate(e.target.value)} options={(templates.data ?? []).filter(t => t.decision_type === type).map(t => ({ value: String(t.id), label: t.name }))} /></Field>
    {templateId && <details><summary className="py-2 cursor-pointer">Template text</summary><pre className="break-words whitespace-pre-wrap text-sm">{templates.data?.find(t => t.id === Number(templateId))?.body_template}</pre></details>}
    <Field label="Outcome" required error={errors.outcome}><Select value={outcome} onChange={e => setOutcome(e.target.value)} options={[{ value: 'approved', label: 'Approved' }, { value: 'approved_with_conditions', label: 'Approved with conditions' }, { value: 'refused', label: 'Refused' }]} /></Field>
    <Field label="Reasons / certificate information" required error={errors.reasons}><Textarea rows={6} value={reasons} onChange={e => setReasons(e.target.value)} /></Field>
    <Field label="Conditions" required={outcome === 'approved_with_conditions'} error={errors.conditions}><Textarea rows={4} value={conditions} onChange={e => setConditions(e.target.value)} /></Field>
    <fieldset><legend className="font-semibold">Evidence — exact versions</legend><p className="text-muted">Latest applicant document versions are selected by default.</p>{(docs.data ?? []).filter(d => d.visibility === 'applicant').map(d => <div key={d.id}>{d.versions.map(v => <Checkbox key={v.id} label={`${d.title} v${v.version}`} checked={evidence.includes(v.id)} onChange={e => setSelected(e.target.checked ? [...evidence, v.id] : evidence.filter(id => id !== v.id))} />)}</div>)}{errors.evidence_version_ids && <p className="text-danger">{errors.evidence_version_ids}</p>}</fieldset>
    {mutation.error && <ErrorAlert error={mutation.error} />}<Button type="submit" loading={mutation.isPending}>Save draft</Button>
  </form>
}
function DecisionActions({ caseId, decision, revision, authority, canPrepare }: { caseId: number; decision: Decision; revision: number; authority: boolean; canPrepare: boolean }) {
  const [reason, setReason] = useState(''), qc = useQueryClient(), toast = useToast()
  const mutation = useMutation({ mutationFn: (action: string) => api.post(`/api/cases/${caseId}/decisions/${decision.id}/${action}`, { expected_revision: revision, reason }), onSuccess: async () => { toast.success('Decision updated.'); await qc.invalidateQueries({ queryKey: ['documents'] }); await qc.invalidateQueries({ queryKey: ['cases'] }) } })
  return <div className="space-y-3 mt-5">{mutation.error && <ErrorAlert error={mutation.error} />}{canPrepare && ['draft', 'returned'].includes(decision.status) && <Button onClick={() => mutation.mutate('submit')} loading={mutation.isPending}>Submit for approval</Button>}
    {decision.status === 'pending_approval' && authority && <><Field label="Reason for returning" error={isApiError(mutation.error) ? mutation.error.fields.reason : undefined}><Textarea value={reason} onChange={e => setReason(e.target.value)} /></Field><div className="flex flex-wrap gap-3"><Button variant="secondary" onClick={() => mutation.mutate('return')} loading={mutation.isPending}>Return for changes</Button><Button onClick={() => mutation.mutate('issue')} loading={mutation.isPending}>Issue decision and PDF</Button></div></>}
  </div>
}
