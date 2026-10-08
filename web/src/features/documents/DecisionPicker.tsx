import { useState } from 'react'
import { useLocation } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Field, Select, Checkbox, ErrorAlert, TextInput } from '@/ui'
import type { FieldComponentProps } from '@/featureTypes'
interface Approval { id: number; decision_type: string; approval_type: string; case_number: string; property_ref: string }
type Answer = { decision_id?: number; decision_ids?: number[] }
const describe = (d: Approval) => `${d.case_number} — ${d.approval_type.replaceAll('_', ' ')}${d.decision_type === 'modification_approval' ? ' (as modified)' : ''} — ${d.property_ref ?? ''}`
function selection(value: unknown) {
  const answer = typeof value === 'object' && value !== null ? value as Answer : {}
  return answer.decision_ids ?? (answer.decision_id ? [answer.decision_id] : [])
}
/**
 * `decision_ref` answers: `{decision_ids:[..]}` when the field allows `multiple`, else `{decision_id}`; both shapes are read.
 * Applicants choose from their own approvals; staff recording an assisted request (`/staff/…`) search the applicant's.
 */
export function DecisionPicker(props: FieldComponentProps) {
  return useLocation().pathname.startsWith('/staff') ? <StaffDecisionPicker {...props} /> : <ApplicantDecisionPicker {...props} />
}
function Choices({ field, value, onChange, error, disabled, approvals, hint, placeholder }: FieldComponentProps & { approvals: Approval[]; hint?: string; placeholder: string }) {
  const selected = selection(value)
  if (field.multiple === true) {
    return <fieldset className="space-y-2" aria-describedby={`${field.key}-hint`}><legend className="font-semibold">{field.label}{field.required ? ' *' : ''}</legend><p id={`${field.key}-hint`} className="text-sm text-muted">{hint ?? 'Choose the development approval, the building approval, or both.'}</p>
      {approvals.map(d => <Checkbox key={d.id} label={describe(d)} disabled={disabled} checked={selected.includes(d.id)} onChange={e => { const ids = e.target.checked ? [...selected, d.id] : selected.filter(id => id !== d.id); onChange(ids.length ? { decision_ids: ids } : undefined) }} />)}
      {error && <p role="alert" className="text-danger">{error}</p>}</fieldset>
  }
  return <Field label={field.label} required={field.required} error={error} hint={hint}><Select value={selected[0] ? String(selected[0]) : ''} disabled={disabled} placeholder={placeholder} options={approvals.map(d => ({ value: String(d.id), label: describe(d) }))} onChange={e => onChange(e.target.value ? { decision_id: Number(e.target.value) } : undefined)} /></Field>
}
function ApplicantDecisionPicker(props: FieldComponentProps) {
  const q = useQuery({ queryKey: ['documents', 'issued-approvals'], queryFn: () => api.get<Approval[]>('/api/my/issued-approvals') })
  const hint = q.data?.length === 0 ? 'You have no current issued approvals. An approval must be issued before you can request a modification.' : props.field.hint
  return <div>{q.error && <ErrorAlert error={q.error} />}<Choices {...props} disabled={props.disabled || q.isPending} approvals={q.data ?? []} hint={hint} placeholder="Choose your issued approval" /></div>
}
function StaffDecisionPicker(props: FieldComponentProps) {
  const [search, setSearch] = useState('')
  const term = search.trim()
  const q = useQuery({ queryKey: ['documents', 'staff-issued-approvals', term], queryFn: () => api.get<Approval[]>('/api/staff/issued-approvals', { query: { q: term } }), enabled: !props.disabled && term.length >= 2 })
  // Keep already-chosen approvals listed while staff search for another one.
  const [known, setKnown] = useState<Approval[]>([])
  const found = q.data ?? []
  const selected = selection(props.value)
  const approvals = [...known.filter(k => selected.includes(k.id) && !found.some(f => f.id === k.id)), ...found]
  const onChange = (v: unknown) => { const ids = selection(v); setKnown(approvals.filter(a => ids.includes(a.id))); props.onChange(v) }
  const missing = selected.filter(id => !approvals.some(a => a.id === id))
  const hint = term.length < 2 ? "Search for the applicant's issued approval first." : q.isSuccess && found.length === 0 ? 'No current issued approvals match on requests you can access.' : props.field.hint
  return <div className="space-y-3">{q.error && <ErrorAlert error={q.error} />}
    {!props.disabled && <Field label="Find the applicant's issued approval" hint="Search by request number, project reference, property or applicant name."><TextInput value={search} onChange={e => setSearch(e.target.value)} /></Field>}
    <Choices {...props} onChange={onChange} disabled={props.disabled || q.isFetching} approvals={approvals} hint={hint} placeholder="Choose the applicant's issued approval" />
    {missing.length > 0 && <p className="text-sm text-muted">Chosen approval decision{missing.length > 1 ? 's' : ''}: {missing.map(id => `#${id}`).join(', ')}</p>}
  </div>
}
