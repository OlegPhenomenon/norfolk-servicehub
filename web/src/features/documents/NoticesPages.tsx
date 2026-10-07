import { useState } from 'react'
import { Link, useParams } from 'react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { PageContainer } from '@/layout/PageContainer'
import { PageHeader, Card, QueryView, EmptyState, DateTime, StatusPill, Field, TextInput, Textarea, Button, ErrorAlert, Alert } from '@/ui'
import type { Exhibition, PublicDetail } from './types'
export function NoticesPage() {
  const q = useQuery({ queryKey: ['documents', 'public-notices'], queryFn: () => api.get<Exhibition[]>('/api/public/exhibitions') })
  return <PageContainer><PageHeader title="Public notices" description="Planning proposals on public exhibition. Read the published materials and send a written submission before the window closes." /><QueryView query={q}>{rows => rows.length ? <div className="grid gap-5 md:grid-cols-2">{rows.map(e => <Card key={e.id} title={e.title} actions={<StatusPill status={e.status} />}><p>{e.summary}</p><p className="mt-4">Comments close <DateTime value={e.closes_at} /></p><Link className="link inline-block py-3" to={`/notices/${e.id}`}>Read notice and published documents</Link></Card>)}</div> : <EmptyState title="No public exhibitions" description="Open and recently closed notices appear here." />}</QueryView></PageContainer>
}
export function NoticePage() {
  const { id } = useParams(), q = useQuery({ queryKey: ['documents', 'public-notice', id], queryFn: () => api.get<PublicDetail>(`/api/public/exhibitions/${id}`) })
  return <PageContainer><QueryView query={q}>{data => <div className="space-y-6 break-words"><PageHeader title={data.exhibition.title} description={data.exhibition.summary} breadcrumbs={[{ label: 'Public notices', to: '/notices' }, { label: data.exhibition.title }]} meta={<StatusPill status={data.exhibition.status} />} /><Alert title="Comment window">Opens <DateTime value={data.exhibition.opens_at} />. Closes <DateTime value={data.exhibition.closes_at} /> (Norfolk Island time).</Alert><Card title="Published documents"><ul>{data.items.map(item => <li key={item.id}><a className="link inline-block py-3" href={item.file_url}>Download {item.title}</a></li>)}</ul></Card>{data.exhibition.status === 'open' ? <SubmissionForm id={Number(id)} /> : <Alert title="The comment window is closed">Written submissions can no longer be sent through this notice.</Alert>}</div>}</QueryView></PageContainer>
}
function SubmissionForm({ id }: { id: number }) {
  const [name, setName] = useState(''), [email, setEmail] = useState(''), [body, setBody] = useState('')
  const m = useMutation({ mutationFn: () => api.post(`/api/public/exhibitions/${id}/submissions`, { name, email, body }) }), errors = isApiError(m.error) ? m.error.fields : {}
  return <Card title="Send a written submission" description="State your grounds if objecting. Your name, email and submission are provided to the planning team and are not listed publicly on this website.">{m.isSuccess ? <Alert tone="success" title="Submission received">The planning team will consider your comments.</Alert> : <form className="space-y-4" onSubmit={e => { e.preventDefault(); m.mutate() }}><Field label="Name" required error={errors.name}><TextInput value={name} onChange={e => setName(e.target.value)} autoComplete="name" /></Field><Field label="Email" required error={errors.email}><TextInput type="email" value={email} onChange={e => setEmail(e.target.value)} autoComplete="email" /></Field><Field label="Written submission" required error={errors.body}><Textarea rows={6} value={body} onChange={e => setBody(e.target.value)} maxLength={10000} /></Field>{m.error && <ErrorAlert error={m.error} />}<Button type="submit" loading={m.isPending}>Send submission</Button></form>}</Card>
}
