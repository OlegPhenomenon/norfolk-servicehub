import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Card, QueryView, EmptyState, DateTime, Badge, Button, Field, TextInput, Textarea, FileInput, Select, Checkbox, ErrorAlert, useToast } from '@/ui'
import type { DocumentRequirement } from '@/api/types'
import type { Document, Version, DecisionList } from './types'

export function DocumentsPanel({ caseId }: { caseId: number }) {
  const q = useQuery({ queryKey: ['documents', 'case', caseId], queryFn: () => api.get<Document[]>(`/api/cases/${caseId}/documents`) })
  const revision = useQuery({ queryKey: ['documents', 'decisions', caseId], queryFn: () => api.get<DecisionList>(`/api/cases/${caseId}/decisions`) })
  const staff = revision.data?.staff ?? false
  const canUpload = revision.data?.can_upload ?? false
  const canComment = revision.data?.can_comment ?? false
  const requirements = revision.data?.document_requirements ?? []
  return <div className="space-y-6 break-words">{revision.error && <ErrorAlert error={revision.error} />}{requirements.length > 0 && <RequirementChecklist requirements={requirements} documents={q.data ?? []} />}{canUpload && <Upload caseId={caseId} staff={staff} revision={revision.data?.revision} requirements={requirements} />}<QueryView query={q}>{docs => docs.length ? <div className="space-y-5 break-words">{docs.map(doc => <Card key={doc.id} title={doc.title} description={[doc.category.replaceAll('_', ' '), requirements.find(r => r.key === doc.requirement_key)?.label].filter(Boolean).join(' · ')} actions={doc.visibility === 'staff' ? <Badge icon="lock">Internal document</Badge> : undefined}>
    <ol className="space-y-4">{doc.versions.map(v => <VersionCard key={v.id} version={v} allVersions={doc.versions} staff={canComment} internal={doc.visibility === 'staff'} revision={revision.data?.revision} />)}</ol>
    {canUpload && !['decision', 'certificate', 'letter'].includes(doc.category) && <Upload caseId={caseId} document={doc} staff={staff} revision={revision.data?.revision} />}
  </Card>)}</div> : <EmptyState title="No documents yet" description="Upload the plans or evidence for this request." />}</QueryView></div>
}
/** The service's document list with requiredness and help, so staff can check what each attachment must contain. */
function RequirementChecklist({ requirements, documents }: { requirements: DocumentRequirement[]; documents: Document[] }) {
  return <Card title="Required and optional documents"><ul className="space-y-3">{requirements.map(r => {
    const attached = documents.filter(d => d.requirement_key === r.key)
    return <li key={r.key}><p className="flex flex-wrap items-center gap-2"><span className="font-semibold">{r.label}</span><Badge tone={r.required ? 'warning' : 'neutral'}>{r.required ? 'Required' : 'Optional'}</Badge><Badge tone={attached.length ? 'success' : 'neutral'}>{attached.length ? `Attached: ${attached.map(d => `${d.title} v${d.versions.at(-1)?.version ?? 1}`).join(', ')}` : 'Not attached'}</Badge></p>{r.help && <p className="text-muted">{r.help}</p>}</li>
  })}</ul></Card>
}
function Upload({ caseId, document, staff, revision, requirements = [] }: { caseId: number; document?: Document; staff?: boolean; revision?: number; requirements?: { key: string; label: string }[] }) {
  const [fileKey, setFileKey] = useState(0)
  const [file, setFile] = useState<File | null>(null), [title, setTitle] = useState(''), [note, setNote] = useState(''), [visibility, setVisibility] = useState('applicant'), [resolves, setResolves] = useState<number[]>([])
  const [category, setCategory] = useState('application'), [requirement, setRequirement] = useState('')
  const qc = useQueryClient(), toast = useToast()
  const comments = document?.versions.flatMap(v => v.comments).filter(c => !c.resolved_by_version_id && c.visibility === 'applicant') ?? []
  const mutation = useMutation({ mutationFn: async () => {
    if (!file) throw new Error('Choose a file to upload.')
    const form = new FormData(); form.append('file', file); form.append('note', note); if (revision !== undefined) form.append('expected_revision', String(revision))
    if (!document) { form.append('title', title || file.name); form.append('category', category); form.append('visibility', visibility); if (requirement) form.append('requirement_key', requirement) }
    resolves.forEach(id => form.append('resolves_comment_ids[]', String(id)))
    return api.upload(document ? `/api/documents/${document.id}/versions` : `/api/cases/${caseId}/documents`, form)
  }, onSuccess: async () => { toast.success(document ? 'New version uploaded. Earlier versions are retained.' : 'Document uploaded.'); setFile(null); setFileKey(k => k + 1); setResolves([]); setNote(''); await qc.invalidateQueries({ queryKey: ['documents'] }); await qc.invalidateQueries({ queryKey: ['cases'] }) } })
  const errors = isApiError(mutation.error) ? mutation.error.fields : {}
  return <form className="mt-5 space-y-4" onSubmit={e => { e.preventDefault(); mutation.mutate() }}>
    <p className="text-sm text-muted">Files are type-checked, not virus-scanned.</p><h3 className="font-semibold">{document ? `This replaces ${document.title} v${document.versions.at(-1)?.version ?? 1}` : 'Upload a document'}</h3>
    {!document && <Field label="Document title" error={errors.title}><TextInput value={title} onChange={e => setTitle(e.target.value)} maxLength={200} /></Field>}
    {!document && <Field label="Category" error={errors.category}><Select value={category} onChange={e => setCategory(e.target.value)} options={[{ value: 'receipt', label: 'Proof of payment' }, {value:'application',label:'Application'},{value:'plans',label:'Plans and drawings'},{value:'evidence',label:'Evidence'},{value:'photo',label:'Photographs'},{value:'supporting',label:'Supporting documents'}]} /></Field>}
    {!document && requirements.length > 0 && <Field label="Document requirement" error={errors.requirement_key}><Select placeholder="Other supporting document" value={requirement} onChange={e => setRequirement(e.target.value)} options={requirements.map(r => ({value:r.key,label:r.label}))} /></Field>}
    <Field label="File" required error={errors.file} hint="PDF, PNG, JPEG or WebP. Maximum 10 MB."><FileInput key={fileKey} accept="application/pdf,image/png,image/jpeg,image/webp" onChange={e => setFile(e.target.files?.[0] ?? null)} /></Field>
    {document && <Field label="What changed?" error={errors.note}><Textarea value={note} onChange={e => setNote(e.target.value)} /></Field>}
    {!document && staff && <Field label="Who can see this?" error={errors.visibility}><Select options={[{ value: 'applicant', label: 'Applicant and staff' }, { value: 'staff', label: 'Staff only' }]} value={visibility} onChange={e => setVisibility(e.target.value)} /></Field>}
    {comments.length > 0 && <fieldset><legend className="font-semibold">Comments resolved by this version</legend>{comments.map(c => <Checkbox key={c.id} label={c.body} checked={resolves.includes(c.id)} onChange={e => setResolves(e.target.checked ? [...resolves, c.id] : resolves.filter(id => id !== c.id))} />)}</fieldset>}
    {mutation.error && <ErrorAlert error={mutation.error} />}<Button type="submit" loading={mutation.isPending}>{document ? 'Upload replacement version' : 'Upload document'}</Button>
  </form>
}
function VersionCard({ version: v, allVersions, staff, internal, revision }: { version: Version; allVersions: Version[]; staff?: boolean; internal?: boolean; revision?: number }) {
  return <li className="border-l-2 border-line pl-4"><div className="flex flex-wrap gap-3 items-center"><strong>Version {v.version}</strong><DateTime value={v.uploaded_at} /><span className="text-muted">{v.uploader}</span><a className="link" href={`/api/document-versions/${v.id}/download`}>Download version {v.version}</a></div>{v.note && <p className="mt-2">{v.note}</p>}
    {v.comments.map(c => <div key={c.id} className={`mt-3 rounded p-3 ${c.visibility === 'internal' ? 'bg-sunken border border-line' : 'bg-primary-50'}`}>
      <div className="flex flex-wrap gap-2">{c.visibility === 'internal' && <Badge icon="lock">Internal comment</Badge>}{c.request_new_version && <Badge tone="warning">{c.resolved_by_version_id ? 'Replacement received' : 'New version requested'}</Badge>}<span>{c.author}</span><DateTime value={c.created_at} /></div><p className="break-words whitespace-pre-wrap">{c.body}</p>{c.resolved_by_version_id && <p className="text-muted">Resolved by version {allVersions.find(v => v.id === c.resolved_by_version_id)?.version ?? 'uploaded later'}</p>}
    </div>)}{staff && <CommentForm versionId={v.id} internal={internal} revision={revision} />}
  </li>
}
function CommentForm({ versionId, internal, revision }: { versionId: number; internal?: boolean; revision?: number }) {
  const [body, setBody] = useState(''), [visibility, setVisibility] = useState(internal ? 'internal' : 'applicant'), [request, setRequest] = useState(false)
  const qc = useQueryClient(), toast = useToast()
  const mutation = useMutation({ mutationFn: () => api.post(`/api/document-versions/${versionId}/comments`, { body, visibility, request_new_version: request, expected_revision: revision }), onSuccess: async () => { setBody(''); toast.success(request ? 'Replacement requested from the applicant.' : 'Comment added.'); await qc.invalidateQueries({ queryKey: ['documents'] }); await qc.invalidateQueries({ queryKey: ['cases'] }) } })
  const errors = isApiError(mutation.error) ? mutation.error.fields : {}
  return <details className="mt-4"><summary className="cursor-pointer py-2 text-primary">Comment or ask for a new version</summary><form className="space-y-3" onSubmit={e => { e.preventDefault(); mutation.mutate() }}>
    <Field label="Comment" required error={errors.body}><Textarea value={body} onChange={e => setBody(e.target.value)} maxLength={5000} /></Field>
    <Field label="Comment visibility" error={errors.visibility}><Select disabled={internal} value={visibility} onChange={e => { setVisibility(e.target.value); if (e.target.value === 'internal') setRequest(false) }} options={[{ value: 'applicant', label: 'Applicant and staff' }, { value: 'internal', label: 'Internal — staff only' }]} /></Field>
    {visibility === 'applicant' && <Checkbox label="Ask for a new version — applicant action required" hint="This request can pause the response clock." checked={request} onChange={e => setRequest(e.target.checked)} />}
    {mutation.error && <ErrorAlert error={mutation.error} />}<Button type="submit" loading={mutation.isPending}>{request ? 'Ask for a new version' : 'Add comment'}</Button>
  </form></details>
}
