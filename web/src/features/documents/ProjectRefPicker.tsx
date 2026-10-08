import { useState } from 'react'
import { useLocation } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { ErrorAlert, Field, Select, TextInput } from '@/ui'
import type { FieldComponentProps } from '@/featureTypes'
interface Project { id: number; reference: string; title: string; property_ref: string; approvals: { id: number; decision_type: string; issued_at: string | null; case_number: string | null }[] }
function describe(p: Project) {
  const approvals = p.approvals.map((a) => `${a.decision_type.replaceAll('_', ' ')} ${a.case_number ?? ''}`.trim())
  return [p.reference, p.title, p.property_ref, approvals.length ? `issued: ${approvals.join(', ')}` : 'no issued approval yet'].filter(Boolean).join(' — ')
}
/**
 * `project_ref` answer: the chosen project's reference, or a typed reference or ID. Applicants pick from their own
 * projects; staff recording an assisted request (`/staff/…`) search the applicant's project by reference.
 */
export function ProjectRefPicker(props: FieldComponentProps) {
  return useLocation().pathname.startsWith('/staff') ? <StaffProjectRefPicker {...props} /> : <ApplicantProjectRefPicker {...props} />
}
function ApplicantProjectRefPicker({ field, value, onChange, error, disabled }: FieldComponentProps) {
  const q = useQuery({ queryKey: ['documents', 'my-building-projects'], queryFn: () => api.get<Project[]>('/api/my/building-projects') })
  const text = typeof value === 'string' || typeof value === 'number' ? String(value) : ''
  const projects = q.data ?? []
  const chosen = projects.find((p) => p.reference === text || String(p.id) === text)
  const help = typeof field.help === 'string' ? field.help : field.hint
  return <fieldset className="space-y-3"><legend className="font-semibold">{field.label}</legend>{q.error && <ErrorAlert error={q.error} />}
    <Field label="Your building projects" required={field.required} error={error} hint={!q.isPending && projects.length === 0 ? 'No building projects are linked to your account yet. Type the reference Council gave you below.' : 'Choose the project this notice is about.'}><Select value={chosen?.reference ?? ''} disabled={disabled || q.isPending} placeholder="Choose a building project" options={projects.map((p) => ({ value: p.reference, label: describe(p) }))} onChange={(e) => onChange(e.target.value || undefined)} /></Field>
    <Field label="Or type the project reference or ID" hint={help}><TextInput value={chosen ? '' : text} disabled={disabled} onChange={(e) => onChange(e.target.value)} /></Field>
  </fieldset>
}
function StaffProjectRefPicker({ field, value, onChange, error, disabled }: FieldComponentProps) {
  const [search, setSearch] = useState('')
  const term = search.trim()
  const q = useQuery({ queryKey: ['documents', 'staff-building-projects', term], queryFn: () => api.get<Project[]>('/api/staff/building-projects', { query: { q: term } }), enabled: !disabled && term.length >= 2 })
  const text = typeof value === 'string' || typeof value === 'number' ? String(value) : ''
  const projects = q.data ?? []
  const chosen = projects.find((p) => p.reference === text || String(p.id) === text)
  return <fieldset className="space-y-3"><legend className="font-semibold">{field.label}</legend>{q.error && <ErrorAlert error={q.error} />}
    {!disabled && <Field label="Find the applicant's building project" hint="Search by project reference, request number, property or applicant name."><TextInput value={search} onChange={(e) => setSearch(e.target.value)} /></Field>}
    {!disabled && term.length >= 2 && <Field label="Matching building projects" hint={q.isSuccess && projects.length === 0 ? 'No matching building projects on requests you can access.' : undefined}><Select value={chosen?.reference ?? ''} disabled={q.isPending} placeholder="Choose the applicant's building project" options={projects.map((p) => ({ value: p.reference, label: describe(p) }))} onChange={(e) => onChange(e.target.value || undefined)} /></Field>}
    <Field label="Applicant's building project reference or ID" required={field.required} error={error} hint="The project the applicant's notice is about, as Council recorded it."><TextInput value={text} disabled={disabled} onChange={(e) => onChange(e.target.value || undefined)} /></Field>
  </fieldset>
}
