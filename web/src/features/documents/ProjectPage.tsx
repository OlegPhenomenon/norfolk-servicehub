import { Link, useLocation, useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { PageHeader, Card, QueryView, Timeline, Badge, StatusPill, EmptyState, DateTime } from '@/ui'
import type { Decision, Document } from './types'
interface ProjectEvent { case_id: number; case_number: string | null; at: string; kind: string; summary: string }
interface Project { reference: string; title: string; property_ref: string; cases: { id: number; number: string; title: string; status: string; created_at: string; links: { case_id: number; kind: string }[]; documents: Document[] }[]; decisions: Decision[]; history: ProjectEvent[] }
function DocumentVersions({ documents }: { documents: Document[] }) {
  if (!documents.length) return null
  return <ul className="mt-2 space-y-1 text-sm">{documents.map(d => <li key={d.id}><span className="font-semibold">{d.title}</span>{d.versions.map(v => <span key={v.id}> · v{v.version} <DateTime value={v.uploaded_at} />{v.comments.filter(c => c.request_new_version).map(c => <Badge key={c.id}>Returned for a new version: {c.body}</Badge>)}</span>)}</li>)}</ul>
}
export function ProjectPage() {
  const { id } = useParams(), area = useLocation().pathname.startsWith('/staff') ? 'staff' : 'my'
  const q = useQuery({ queryKey: ['documents', 'project', id], queryFn: () => api.get<Project>(`/api/building-projects/${id}`) })
  return <QueryView query={q}>{p => <div className="space-y-6 break-words"><PageHeader eyebrow={p.reference} title={p.title} description={p.property_ref} /><Card title="Project requests"><Timeline items={p.cases.map(c => ({ id: c.id, at: c.created_at, title: c.title, body: <div><Link className="link" to={`/${area}/cases/${c.id}`}>{c.number}</Link> <StatusPill status={c.status} />{c.links.map(l => <p key={`${l.case_id}-${l.kind}`}><Link className="link" to={`/${area}/cases/${l.case_id}`}>{l.kind.replaceAll('_', ' ')} request {l.case_id}</Link></p>)}<DocumentVersions documents={c.documents} /></div> }))} /></Card>
    <Card title="Decision history">{!p.decisions.length && <EmptyState title="No decisions yet" description="Issued approvals and their evidence will appear here after Council review." />}{p.decisions.map(d => <section key={d.id} className="border-b border-line py-4"><h3 className="font-semibold">{d.decision_type.replaceAll('_', ' ')} · {d.outcome.replaceAll('_', ' ')}</h3><StatusPill status={d.status} />{d.issued_at && <p>Issued <DateTime value={d.issued_at} /></p>}{d.supersedes_decision_id && <p>Supersedes approval #{d.supersedes_decision_id}</p>}<div className="flex flex-wrap gap-2 mt-3">{d.evidence.map(e => <Badge key={e.id}>Decision based on {e.title} v{e.version}</Badge>)}</div>{d.output_document_version_id && <a className="link inline-block py-3" href={`/api/document-versions/${d.output_document_version_id}/download`}>Download issued decision</a>}</section>)}</Card>
    <Card title="Project history">{!p.history.length ? <EmptyState title="No history yet" /> : <Timeline items={p.history.map((e, i) => ({ id: i, at: e.at, title: e.summary, body: <Link className="link" to={`/${area}/cases/${e.case_id}`}>{e.case_number ?? `Request ${e.case_id}`}</Link> }))} />}</Card>
  </div>}</QueryView>
}
