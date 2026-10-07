import { Table, DateTime, StatusPill, EmptyState, ButtonLink } from '@/ui'
import type { RecordCase } from './types'
export function CasesTable({ cases }: { cases: RecordCase[] }) {
  return <Table caption="Requests" rows={cases} rowKey={r => r.id} empty={<EmptyState title="No requests match" description="Try another period or search." />} columns={[
    { key: 'case', header: 'Request', cell: r => <ButtonLink variant="ghost" to={`/staff/cases/${r.id}`}>{r.number ?? `Request ${r.id}`} — {r.title}</ButtonLink> },
    { key: 'person', header: 'Applicant', cell: r => r.applicant_name ?? '—' },
    { key: 'status', header: 'Status', cell: r => <StatusPill status={r.status} /> },
    { key: 'closed', header: 'Closed', cell: r => r.closed_at ? <DateTime value={r.closed_at} format="date" /> : '—' },
  ]} />
}
