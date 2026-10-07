import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { Answers, AnswerValue, FieldDef, ServiceDefinition } from '@/api/types'
import { api } from '@/api/client'
import { fieldTypes } from '@/registry'
import { Alert, Checkbox, DateTime, EmptyState, ErrorAlert, Field, FileInput, Select, Textarea, TextInput } from '@/ui'

interface Props { definition: ServiceDefinition; answers: Answers; onChange: (answers: Answers) => void; errors?: Record<string, string>; caseId?: number; disabled?: boolean; review?: boolean }
export function FormRenderer({ definition, answers, onChange, errors = {}, caseId, disabled, review }: Props) {
  // Evaluate conditions against earlier visible answers, matching the authoritative server.
  const visible: Answers = {}
  const fields = definition.fields.filter((f) => {
    if (f.show_if && visible[f.show_if.field] !== f.show_if.equals) return false
    if (answers[f.key] !== undefined) visible[f.key] = answers[f.key]!
    return true
  })
  const change = (key: string, value: unknown) => onChange({ ...answers, [key]: value as AnswerValue })
  if (review) return <div className="space-y-4">{fields.map((f) => <div key={f.key}><h3 className="font-semibold">{f.label}</h3><p className="text-muted break-words">{readable(answers[f.key])}</p></div>)}<Alert title="Before you submit">Check your details and documents. Receiving your request does not mean it is approved.</Alert></div>
  return <div className="space-y-5">{fields.map((f) => {
    const Custom = fieldTypes[f.type]
    if (Custom) return <Custom key={f.key} field={f} value={answers[f.key]} onChange={(v) => change(f.key, v)} error={errors[f.key]} disabled={disabled} />
    if (f.type === 'checkbox') return <Checkbox key={f.key} label={f.label} checked={answers[f.key] === true} onChange={(e) => change(f.key, e.target.checked)} error={errors[f.key]} disabled={disabled} />
    if (f.type === 'multiselect') return <fieldset key={f.key} className="space-y-2"><legend className="font-semibold">{f.label}</legend>{f.options?.map((o) => <Checkbox key={o.value} label={o.label} checked={Array.isArray(answers[f.key]) && (answers[f.key] as string[]).includes(o.value)} onChange={(e) => { const values = Array.isArray(answers[f.key]) ? answers[f.key] as string[] : []; change(f.key, e.target.checked ? [...values, o.value] : values.filter((v) => v !== o.value)) }} disabled={disabled} />)}{errors[f.key] && <p role="alert" className="text-danger">{errors[f.key]}</p>}</fieldset>
    if (f.type === 'location') return <LocationField key={f.key} field={f} value={answers[f.key]} onChange={(v) => change(f.key, v)} error={errors[f.key]} disabled={disabled} />
    if (['booking_slot', 'equipment_request', 'decision_ref'].includes(f.type)) return <Alert key={f.key} tone="warning" title={f.label}>The service widget is unavailable. Reload after the service module is installed.</Alert>
    return <Field key={f.key} label={f.label} required={f.required} hint={typeof f.help === 'string' ? f.help : f.hint} error={errors[f.key]}>{f.type === 'select' ? <Select options={f.options ?? []} placeholder="Choose an option" value={String(answers[f.key] ?? '')} onChange={(e) => change(f.key, e.target.value)} disabled={disabled} /> : f.type === 'textarea' ? <Textarea value={String(answers[f.key] ?? '')} onChange={(e) => change(f.key, e.target.value)} maxLength={f.max_length} disabled={disabled} /> : <TextInput type={f.type === 'phone' ? 'tel' : ['number', 'email', 'date', 'time'].includes(f.type) ? f.type : 'text'} value={String(answers[f.key] ?? '')} onChange={(e) => change(f.key, f.type === 'number' && e.target.value !== '' ? Number(e.target.value) : e.target.value)} maxLength={f.max_length} min={f.min} max={f.max} disabled={disabled} />}</Field>
  })}{definition.documents.length > 0 && <section aria-label="Documents to attach" className="space-y-4"><h2 className="font-serif text-xl">Supporting documents</h2>{caseId ? <DocumentSlots caseId={caseId} definition={definition} errors={errors} disabled={disabled} /> : <p className="text-muted">{definition.documents.map((d) => `${d.label}${d.required ? ' (required)' : ''}`).join('; ')}. Save the request before uploading documents.</p>}</section>}</div>
}
function readable(value: unknown): string {
  if (value === undefined || value === '') return 'Not provided'
  if (typeof value === 'boolean') return value ? 'Yes' : 'No'
  if (Array.isArray(value)) return value.join(', ')
  if (value && typeof value === 'object') return Object.entries(value).map(([key, value]) => `${key.replaceAll('_', ' ')}: ${String(value)}`).join('; ')
  return String(value)
}
function LocationField({ field, value, onChange, error, disabled }: { field: FieldDef; value: unknown; onChange: (v: unknown) => void; error?: string; disabled?: boolean }) {
  const v = (value ?? { lat: -29.04, lng: 167.95, description: '' }) as { lat: number; lng: number; description: string }
  return <fieldset className="space-y-3"><legend className="font-semibold">{field.label}</legend><Field label="Road and nearest landmark" required error={error}><Textarea value={v.description} onChange={(e) => onChange({ ...v, description: e.target.value })} disabled={disabled} /></Field><div className="grid gap-3 sm:grid-cols-2"><Field label="Latitude" required><TextInput type="number" step="any" min={-90} max={90} value={v.lat} onChange={(e) => onChange({ ...v, lat: Number(e.target.value) })} disabled={disabled} /></Field><Field label="Longitude" required><TextInput type="number" step="any" min={-180} max={180} value={v.lng} onChange={(e) => onChange({ ...v, lng: Number(e.target.value) })} disabled={disabled} /></Field></div></fieldset>
}
interface DocumentRow { id: number; title: string; requirement_key?: string; created_at?: string; latest_version?: { id: number }; versions?: { id: number }[] }
export function DocumentSlots({ caseId, definition, errors = {}, disabled }: { caseId: number; definition: ServiceDefinition; errors?: Record<string, string>; disabled?: boolean }) {
  const client = useQueryClient()
  const [slot, setSlot] = useState<string | null>(null)
  const docs = useQuery({ queryKey: ['services', 'documents', caseId], queryFn: () => api.get<{ items: DocumentRow[] } | DocumentRow[]>(`/api/cases/${caseId}/documents`) })
  const upload = useMutation({ mutationFn: ({ key, file }: { key: string; file: File }) => api.upload(`/api/cases/${caseId}/documents`, { file, fields: { requirement_key: key } }), onSuccess: () => client.invalidateQueries({ queryKey: ['services', 'documents', caseId] }) })
  const rows = Array.isArray(docs.data) ? docs.data : docs.data?.items ?? []
  return <div className="space-y-4">{docs.error && <ErrorAlert error={docs.error} />}{upload.error && <ErrorAlert error={upload.error} />}{definition.documents.map((d) => <div key={d.key} className="rounded-lg border border-line p-4"><Field label={d.label} required={d.required} error={errors[`documents.${d.key}`]} hint="PDF, PNG or JPEG, up to 10 MB."><FileInput accept={d.accept.join(',')} disabled={disabled || upload.isPending} onChange={(e) => { const file = e.target.files?.[0]; if (file) { setSlot(d.key); upload.mutate({ key: d.key, file }) } }} /></Field>{rows.filter((r) => r.requirement_key === d.key).map((r) => <p key={r.id} className="mt-2 text-sm">Attached: {r.title} {r.created_at && <DateTime value={r.created_at} />}</p>)}{upload.isPending && slot === d.key && <p role="status">Uploading…</p>}</div>)}{!definition.documents.length && <EmptyState title="No documents needed" />}<p aria-live="polite" className="text-sm text-pine">{upload.isSuccess ? 'Document attached.' : ''}</p></div>
}
