import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Badge, Button, Card, DateTime, EmptyState, ErrorAlert, Field, QueryView, Textarea, TextInput, useToast } from '@/ui'

interface LetterStep { step_key: string; step_label: string; letter_type: string; label: string; current: boolean; reached: boolean; issued: boolean; can_issue: boolean }
interface IssuedLetter { id: number; letter_type: string; title: string; document_version_id: number; issued_at: string; issued_by_name: string | null; step_key: string | null }
interface Letters { revision: number; can_issue: boolean; steps: LetterStep[]; issued: IssuedLetter[] }

export function LettersPanel({ caseId }: { caseId: number }) {
  const q = useQuery({ queryKey: ['documents', 'letters', caseId], queryFn: () => api.get<Letters>(`/api/cases/${caseId}/letters`) })
  return <QueryView query={q}>{(d) => {
    const steps = d.steps.filter((s) => s.letter_type !== 'road_response')
    return <div className="space-y-5">
      {steps.map((s) => <Card key={s.step_key} title={`Step: ${s.step_label}`} actions={s.issued ? <Badge tone="success">Issued</Badge> : s.current ? <Badge tone="warning">Waiting for this letter</Badge> : s.reached ? <Badge>Not issued</Badge> : <Badge>Not yet reached</Badge>}>
        <p>This step completes when the {s.label} letter is issued at this step. The applicant can download it from Documents.</p>
        {!s.current && !s.reached && <p className="mt-2 text-sm">The letter can be issued once the case reaches this step.</p>}
        {s.can_issue && <LetterForm caseId={caseId} revision={d.revision} step={s} />}
      </Card>)}
      <Card title="Issued letters">{d.issued.length ? <ul className="space-y-3">{d.issued.map((l) => {
        const step = d.steps.find((s) => s.step_key === l.step_key)?.step_label
        return <li key={l.id}><a className="link" href={`/api/document-versions/${l.document_version_id}/download`}>{l.title}</a> — {l.letter_type.replaceAll('_', ' ')}{step && ` (step: ${step})`}, <DateTime value={l.issued_at} />{l.issued_by_name && ` by ${l.issued_by_name}`}</li>
      })}</ul> : <EmptyState title="No letters issued yet" />}</Card>
    </div>
  }}</QueryView>
}

function LetterForm({ caseId, revision, step }: { caseId: number; revision: number; step: LetterStep }) {
  const [title, setTitle] = useState(step.label[0]!.toUpperCase() + step.label.slice(1))
  const [body, setBody] = useState('')
  const client = useQueryClient(), toast = useToast()
  const issue = useMutation({
    mutationFn: () => api.post(`/api/cases/${caseId}/letters`, { letter_type: step.letter_type, title, body, expected_revision: revision }),
    onSuccess: async () => { toast.success('Response letter issued.'); setBody(''); await client.invalidateQueries({ queryKey: ['documents'] }); await client.invalidateQueries({ queryKey: ['cases'] }) },
  })
  const errors = isApiError(issue.error) ? issue.error.fields : {}
  return <form className="mt-4 space-y-4" onSubmit={(e) => { e.preventDefault(); issue.mutate() }}>
    <Field label="Letter title" required error={errors.title}><TextInput value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} /></Field>
    <Field label="Response for the applicant" required hint="The response becomes an issued PDF letter in the case documents." error={errors.body}><Textarea rows={6} value={body} maxLength={20000} onChange={(e) => setBody(e.target.value)} /></Field>
    {issue.error && <ErrorAlert error={issue.error} />}
    <Button type="submit" loading={issue.isPending}>Issue {step.label} letter</Button>
  </form>
}
