import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { Answers, AnswerValue, FieldDef, GroupRow, ServiceDefinition, ShowIf } from '@/api/types'
import { api } from '@/api/client'
import { humanize } from '@/ui/status'
import { fieldTypes } from '@/registry'
import { Alert, Button, Checkbox, DateTime, EmptyState, ErrorAlert, Field, FileInput, Select, Table, Textarea, TextInput } from '@/ui'

/** Same rule as the server (`ShowIf::matches`): equality, or "contains" when the earlier answer is a multiselect. */
function showIfMatches(condition: ShowIf, answer: unknown): boolean {
  return Array.isArray(answer) ? (answer as unknown[]).includes(condition.equals) : answer === condition.equals
}

interface Props { definition: ServiceDefinition; answers: Answers; onChange: (answers: Answers) => void; errors?: Record<string, string>; caseId?: number; disabled?: boolean; review?: boolean; submitted?: boolean }
export function FormRenderer({ definition, answers, onChange, errors = {}, caseId, disabled, review, submitted }: Props) {
  // Evaluate conditions against earlier visible answers, matching the authoritative server.
  const visible: Answers = {}
  const fields = definition.fields.filter((f) => {
    if (f.show_if && !showIfMatches(f.show_if, visible[f.show_if.field])) return false
    if (answers[f.key] !== undefined) visible[f.key] = answers[f.key]!
    return true
  })
  const change = (key: string, value: unknown) => onChange({ ...answers, [key]: value as AnswerValue })
  if (review) return <div className="space-y-4">{fields.map((f) => <div key={f.key}><h3 className="font-semibold">{f.label}</h3><p className="text-muted break-words">{readable(answers[f.key], f)}</p></div>)}{!submitted && <Alert title="Before you submit">Check your details and documents. Receiving your request does not mean it is approved.</Alert>}</div>
  return <div className="space-y-5">{fields.map((f) => {
    const Custom = fieldTypes[f.type]
    if (Custom) return <Custom key={f.key} field={f} value={answers[f.key]} onChange={(v) => change(f.key, v)} error={errors[f.key]} disabled={disabled} />
    if (f.type === 'checkbox') return <Checkbox key={f.key} label={f.label} checked={answers[f.key] === true} onChange={(e) => change(f.key, e.target.checked)} error={errors[f.key]} disabled={disabled} />
    if (f.type === 'multiselect') return <fieldset key={f.key} className="space-y-2"><legend className="font-semibold">{f.label}</legend>{f.options?.map((o) => <Checkbox key={o.value} label={o.label} checked={Array.isArray(answers[f.key]) && (answers[f.key] as string[]).includes(o.value)} onChange={(e) => { const values = Array.isArray(answers[f.key]) ? answers[f.key] as string[] : []; change(f.key, e.target.checked ? [...values, o.value] : values.filter((v) => v !== o.value)) }} disabled={disabled} />)}{errors[f.key] && <p role="alert" className="text-danger">{errors[f.key]}</p>}</fieldset>
    if (f.type === 'group') return <GroupField key={f.key} field={f} value={answers[f.key]} onChange={(v) => change(f.key, v)} errors={errors} disabled={disabled} />
    if (f.type === 'location') return <LocationField key={f.key} field={f} value={answers[f.key]} onChange={(v) => change(f.key, v)} error={errors[f.key]} disabled={disabled} />
    if (['booking_slot', 'equipment_request', 'decision_ref'].includes(f.type)) return <Alert key={f.key} tone="warning" title={f.label}>The service widget is unavailable. Reload after the service module is installed.</Alert>
    return <Field key={f.key} label={f.label} required={f.required} hint={typeof f.help === 'string' ? f.help : f.hint} error={errors[f.key]}>{f.type === 'select' ? <Select options={f.options ?? []} placeholder="Choose an option" value={String(answers[f.key] ?? '')} onChange={(e) => change(f.key, e.target.value)} disabled={disabled} /> : f.type === 'textarea' ? <Textarea value={String(answers[f.key] ?? '')} onChange={(e) => change(f.key, e.target.value)} maxLength={f.max_length} disabled={disabled} /> : <TextInput type={f.type === 'phone' ? 'tel' : ['number', 'email', 'date', 'time'].includes(f.type) ? f.type : 'text'} value={String(answers[f.key] ?? '')} onChange={(e) => change(f.key, f.type === 'number' && e.target.value !== '' ? Number(e.target.value) : e.target.value)} maxLength={f.max_length} min={f.min} max={f.max} disabled={disabled} />}</Field>
  })}{definition.documents.length > 0 && <section aria-label="Documents to attach" className="space-y-4"><h2 className="font-serif text-xl">Supporting documents</h2>{caseId ? <DocumentSlots caseId={caseId} definition={definition} errors={errors} disabled={disabled} /> : <p className="text-muted">{definition.documents.map((d) => `${d.label}${d.required ? ' (required)' : ''}`).join('; ')}. Save the request before uploading documents.</p>}</section>}</div>
}
function readable(value: unknown, field: FieldDef): ReactNode {
  if (value === undefined || value === null || value === '') return 'Not provided'
  if (field.type === 'group') return <GroupTable field={field} rows={Array.isArray(value) ? value as GroupRow[] : []} />
  if (typeof value === 'boolean') return value ? 'Yes' : 'No'
  const optionLabel = (v: unknown) => field.options?.find((o) => o.value === v)?.label ?? String(v)
  if (Array.isArray(value)) return value.map(optionLabel).join(', ')
  if (value && typeof value === 'object') {
    const labels: Record<string, string> = { unit_code: 'Space', start_at: 'Starts', end_at: 'Ends', attendees: 'Guests', decision_id: 'Approval reference', site_text: 'Work site' }
    const spaces: Record<string, string> = { 'rawson-main': 'Main hall', 'rawson-supper': 'Supper room', 'rawson-whole': 'Whole hall' }
    return Object.entries(value).map(([key, item], i) => <span key={key}>{i > 0 && '; '}{labels[key] ?? humanize(key)}: {key === 'unit_code' ? spaces[String(item)] ?? humanize(String(item)) : ['start_at', 'end_at'].includes(key) ? <DateTime value={String(item)} /> : key === 'preferred_date' ? <DateTime value={String(item)} format="date" /> : String(item)}</span>)
  }
  if (field.type === 'date') return <DateTime value={String(value)} format="date" />
  return optionLabel(value)
}
/** Review table of a `group` answer: one row per entry, one column per configured column. */
function GroupTable({ field, rows }: { field: FieldDef; rows: GroupRow[] }) {
  const columns = field.columns ?? []
  return <Table caption={field.label} hideCaption dense rows={rows.map((row, i) => ({ row, i }))} rowKey={(r) => r.i} empty="Not provided" columns={[{ key: '#', header: '#', cell: (r) => r.i + 1 }, ...columns.map((c) => ({ key: c.key, header: c.label, cell: (r: { row: GroupRow }) => <span className="break-words whitespace-pre-wrap">{readable(r.row[c.key], c)}</span> }))]} />
}
/** Repeating rows (`group`): add/remove rows; each cell is validated by the server as `<field>.<row>.<column>`. */
function GroupField({ field, value, onChange, errors, disabled }: { field: FieldDef; value: unknown; onChange: (v: GroupRow[]) => void; errors: Record<string, string>; disabled?: boolean }) {
  const rows = Array.isArray(value) ? value as GroupRow[] : []
  const max = field.max_items
  const set = (i: number, key: string, cell: string | number | boolean) => onChange(rows.map((r, n) => n === i ? { ...r, [key]: cell } : r))
  const hint = typeof field.help === 'string' ? field.help : field.hint
  return <fieldset className="space-y-3" aria-describedby={hint ? `${field.key}-help` : undefined}><legend className="font-semibold">{field.label}{field.required && <span className="text-danger" aria-hidden="true"> *</span>}</legend>{hint && <p id={`${field.key}-help`} className="text-muted">{hint}</p>}
    {rows.map((row, i) => <fieldset key={i} className="rounded-lg border border-line p-4 space-y-3"><legend className="px-1 font-semibold">{field.label}: row {i + 1}</legend><div className="grid gap-3 sm:grid-cols-2">{(field.columns ?? []).map((c) => {
      const error = errors[`${field.key}.${i}.${c.key}`]
      if (c.type === 'checkbox') return <Checkbox key={c.key} className="sm:col-span-2" label={c.label} checked={row[c.key] === true} onChange={(e) => set(i, c.key, e.target.checked)} error={error} disabled={disabled} />
      return <Field key={c.key} className={c.type === 'textarea' ? 'sm:col-span-2' : undefined} label={c.label} required={c.required} hint={typeof c.help === 'string' ? c.help : c.hint} error={error}>{c.type === 'select' ? <Select options={c.options ?? []} placeholder="Choose an option" value={String(row[c.key] ?? '')} onChange={(e) => set(i, c.key, e.target.value)} disabled={disabled} /> : c.type === 'textarea' ? <Textarea value={String(row[c.key] ?? '')} onChange={(e) => set(i, c.key, e.target.value)} maxLength={c.max_length} disabled={disabled} /> : <TextInput type={c.type === 'phone' ? 'tel' : ['number', 'email', 'date'].includes(c.type) ? c.type : 'text'} value={String(row[c.key] ?? '')} onChange={(e) => set(i, c.key, c.type === 'number' && e.target.value !== '' ? Number(e.target.value) : e.target.value)} maxLength={c.max_length} min={c.min} max={c.max} disabled={disabled} />}</Field>
    })}</div>{!disabled && <Button variant="danger-outline" onClick={() => onChange(rows.filter((_, n) => n !== i))}>Remove {field.label.toLowerCase()} row {i + 1}</Button>}</fieldset>)}
    {!rows.length && <p className="text-muted">No rows yet.</p>}
    {!disabled && (max === undefined || rows.length < max) && <Button variant="secondary" onClick={() => onChange([...rows, {}])}>Add {field.label.toLowerCase()} row</Button>}
    {errors[field.key] && <p role="alert" className="text-danger">{errors[field.key]}</p>}
  </fieldset>
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
  return <div className="space-y-4">{docs.error && <ErrorAlert error={docs.error} />}{upload.error && <ErrorAlert error={upload.error} />}{definition.documents.map((d) => <div key={d.key} className="rounded-lg border border-line p-4"><Field label={d.label} required={d.required} error={errors[`documents.${d.key}`]} hint={d.help ? `${d.help} PDF, PNG or JPEG, up to 10 MB.` : 'PDF, PNG or JPEG, up to 10 MB.'}><FileInput accept={d.accept.join(',')} disabled={disabled || upload.isPending} onChange={(e) => { const file = e.target.files?.[0]; if (file) { setSlot(d.key); upload.mutate({ key: d.key, file }) } }} /></Field>{rows.filter((r) => r.requirement_key === d.key).map((r) => <p key={r.id} className="mt-2 text-sm">Attached: {r.title} {r.created_at && <DateTime value={r.created_at} />}</p>)}{upload.isPending && slot === d.key && <p role="status">Uploading…</p>}</div>)}{!definition.documents.length && <EmptyState title="No documents needed" />}<p aria-live="polite" className="text-sm text-pine">{upload.isSuccess ? 'Document attached.' : ''}</p></div>
}
