import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Field, Select, ErrorAlert } from '@/ui'
import type { FieldComponentProps } from '@/featureTypes'
export function DecisionPicker({ field, value, onChange, error, disabled }: FieldComponentProps) {
  const q = useQuery({ queryKey: ['documents', 'issued-approvals'], queryFn: () => api.get<{ id: number; decision_type: string; case_number: string; property_ref: string }[]>('/api/my/issued-approvals') })
  const id = typeof value === 'object' && value !== null && 'decision_id' in value ? String(value.decision_id) : ''
  return <div>{q.error && <ErrorAlert error={q.error} />}<Field label={field.label} required={field.required} error={error} hint={q.data?.length === 0 ? 'You have no issued approvals. An approval must be issued before you can request a modification.' : field.hint}><Select value={id} disabled={disabled || q.isPending} placeholder="Choose your issued approval" options={(q.data ?? []).map(d => ({ value: String(d.id), label: `${d.case_number} — ${d.decision_type.replaceAll('_', ' ')} — ${d.property_ref ?? ''}` }))} onChange={e => onChange(e.target.value ? { decision_id: Number(e.target.value) } : undefined)} /></Field></div>
}
