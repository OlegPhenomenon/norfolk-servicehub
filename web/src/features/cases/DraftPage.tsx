import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router'
import { api, isApiError, newIdempotencyKey } from '@/api/client'
import type { Answers } from '@/api/types'
import { Alert, Button, Card, ErrorAlert, PageHeader, QueryView } from '@/ui'
import { FormRenderer } from '../services/FormRenderer'
import type { DraftDetail, DraftSaved } from './types'
export function DraftPage() {
  const { id } = useParams()
  const q = useQuery({ queryKey: ['cases', 'draft', id], queryFn: () => api.get<DraftDetail>(`/api/cases/${id}/draft`) })
  return <QueryView query={q}>{(data) => <DraftEditor key={data.case.id} data={data} />}</QueryView>
}
function DraftEditor({ data }: { data: DraftDetail }) {
  const navigate = useNavigate(); const client = useQueryClient(); const [answers, setAnswers] = useState(data.answers); const [review, setReview] = useState(false)
  const [saved, setSaved] = useState('Saved'); const [saveError, setSaveError] = useState<unknown>(null)
  // The server moves a draft off a replaced service version on save/submit; reload the current form when it does.
  const [formUpdated, setFormUpdated] = useState(false)
  const chain = useRef<Promise<unknown>>(Promise.resolve()); const changed = useRef(false); const key = useRef(newIdempotencyKey())
  const persist = useCallback((snapshot: Answers) => { const operation = chain.current.catch(() => undefined).then(() => api.put<DraftSaved>(`/api/cases/${data.case.id}/draft`, { answers: snapshot })).then((result) => { if (result.form_updated) { setFormUpdated(true); void client.invalidateQueries({ queryKey: ['cases', 'draft'] }) } return result }); chain.current = operation; return operation }, [data.case.id, client])
  useEffect(() => {
    if (!changed.current) return
    const timer = setTimeout(() => {
      setSaved('Saving…'); setSaveError(null)
      persist(answers).then(() => setSaved('Saved')).catch((e: unknown) => { setSaved('Could not save'); setSaveError(e) })
    }, 600)
    return () => clearTimeout(timer)
  }, [answers, persist])
  const submit = useMutation({ mutationFn: async () => {
    await persist(answers)
    return api.post<{ id: number; number: string }>(`/api/cases/${data.case.id}/submit`, undefined, { idempotencyKey: key.current })
  }, onError: () => { setReview(false); void client.invalidateQueries({ queryKey: ['cases', 'draft'] }) }, onSuccess: (result) => navigate(`/my/cases/${result.id}?received=1`) })
  const errors = isApiError(submit.error) ? submit.error.fields : {}
  const change = (a: Answers) => { changed.current = true; setSaved('Unsaved changes'); setAnswers(a); key.current = newIdempotencyKey() }
  return <><PageHeader title={data.case.title} eyebrow="Request draft" description="Your progress is saved automatically. You can return to it from My requests." /><Card title={review ? 'Review your request' : 'Your details'}><p aria-live="polite" className="mb-4 text-sm text-muted">{saved}</p>{(formUpdated || data.form_updated) && <div className="mb-4"><Alert tone="info" title="This form was updated since you started">Please check your answers before submitting.</Alert></div>}{saveError ? <ErrorAlert error={saveError} /> : null}{submit.error && <ErrorAlert error={submit.error} />}<FormRenderer definition={data.definition} answers={answers} onChange={change} caseId={data.case.id} errors={errors} review={review} disabled={submit.isPending} /><div className="mt-6 flex flex-wrap gap-3">{review ? <><Button variant="secondary" onClick={() => setReview(false)} disabled={submit.isPending}>Edit details</Button><Button loading={submit.isPending} onClick={() => submit.mutate()}>Submit request</Button></> : <Button onClick={() => setReview(true)}>Review before submitting</Button>}</div><Alert title="This is a draft">You will receive a reference number after submitting. Your booking is confirmed separately.</Alert></Card></>
}
