import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { ErrorAlert, Field, Select, TextInput } from '@/ui'
import type { FieldComponentProps } from '@/featureTypes'
interface MyProject { id: number; reference: string; title: string; property_ref: string; approvals: { id: number; decision_type: string; issued_at: string | null; case_number: string | null }[] }
function describe(p: MyProject) {
  const approvals = p.approvals.map((a) => `${a.decision_type.replaceAll('_', ' ')} ${a.case_number ?? ''}`.trim())
  return [p.reference, p.title, p.property_ref, approvals.length ? `issued: ${approvals.join(', ')}` : 'no issued approval yet'].filter(Boolean).join(' — ')
}
/** `project_ref` answer: the chosen project's reference, or a typed reference or ID. */
export function ProjectRefPicker({ field, value, onChange, error, disabled }: FieldComponentProps) {
  const q = useQuery({ queryKey: ['documents', 'my-building-projects'], queryFn: () => api.get<MyProject[]>('/api/my/building-projects') })
  const text = typeof value === 'string' || typeof value === 'number' ? String(value) : ''
  const projects = q.data ?? []
  const chosen = projects.find((p) => p.reference === text || String(p.id) === text)
  const help = typeof field.help === 'string' ? field.help : field.hint
  return <fieldset className="space-y-3"><legend className="font-semibold">{field.label}</legend>{q.error && <ErrorAlert error={q.error} />}
    <Field label="Your building projects" required={field.required} error={error} hint={!q.isPending && projects.length === 0 ? 'No building projects are linked to your account yet. Type the reference Council gave you below.' : 'Choose the project this notice is about.'}><Select value={chosen?.reference ?? ''} disabled={disabled || q.isPending} placeholder="Choose a building project" options={projects.map((p) => ({ value: p.reference, label: describe(p) }))} onChange={(e) => onChange(e.target.value || undefined)} /></Field>
    <Field label="Or type the project reference or ID" hint={help}><TextInput value={chosen ? '' : text} disabled={disabled} onChange={(e) => onChange(e.target.value)} /></Field>
  </fieldset>
}
