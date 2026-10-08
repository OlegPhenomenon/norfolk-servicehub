import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Field, Select, Checkbox, ErrorAlert } from '@/ui'
import type { FieldComponentProps } from '@/featureTypes'
interface Approval { id: number; decision_type: string; approval_type: string; case_number: string; property_ref: string }
/** `decision_ref` answers: `{decision_ids:[..]}` when the field allows `multiple`, else `{decision_id}`; both shapes are read. */
export function DecisionPicker({ field, value, onChange, error, disabled }: FieldComponentProps) {
  const q = useQuery({ queryKey: ['documents', 'issued-approvals'], queryFn: () => api.get<Approval[]>('/api/my/issued-approvals') })
  const answer = typeof value === 'object' && value !== null ? value as { decision_id?: number; decision_ids?: number[] } : {}
  const selected = answer.decision_ids ?? (answer.decision_id ? [answer.decision_id] : [])
  const describe = (d: Approval) => `${d.case_number} — ${d.approval_type.replaceAll('_', ' ')}${d.decision_type === 'modification_approval' ? ' (as modified)' : ''} — ${d.property_ref ?? ''}`
  const hint = q.data?.length === 0 ? 'You have no current issued approvals. An approval must be issued before you can request a modification.' : field.hint
  if (field.multiple === true) {
    return <div>{q.error && <ErrorAlert error={q.error} />}<fieldset className="space-y-2" aria-describedby={`${field.key}-hint`}><legend className="font-semibold">{field.label}{field.required ? ' *' : ''}</legend><p id={`${field.key}-hint`} className="text-sm text-muted">{hint ?? 'Choose the development approval, the building approval, or both.'}</p>
      {(q.data ?? []).map(d => <Checkbox key={d.id} label={describe(d)} disabled={disabled} checked={selected.includes(d.id)} onChange={e => { const ids = e.target.checked ? [...selected, d.id] : selected.filter(id => id !== d.id); onChange(ids.length ? { decision_ids: ids } : undefined) }} />)}
      {error && <p role="alert" className="text-danger">{error}</p>}</fieldset></div>
  }
  return <div>{q.error && <ErrorAlert error={q.error} />}<Field label={field.label} required={field.required} error={error} hint={hint}><Select value={selected[0] ? String(selected[0]) : ''} disabled={disabled || q.isPending} placeholder="Choose your issued approval" options={(q.data ?? []).map(d => ({ value: String(d.id), label: describe(d) }))} onChange={e => onChange(e.target.value ? { decision_id: Number(e.target.value) } : undefined)} /></Field></div>
}
