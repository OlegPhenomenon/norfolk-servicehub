import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { RequireStaff } from '@/auth/RequireStaff'
import { Button, ButtonLink, DateTime, Dialog, EmptyState, Money, PageHeader, QueryView, Table } from '@/ui'
import type { DepositRow } from './types'
import { DepositForm } from './DepositForm'
export function DepositsPage() {
  const q = useQuery({ queryKey: ['finance', 'deposits'], queryFn: () => api.get<DepositRow[]>('/api/finance/deposits', { query: { status: 'awaiting_decision' } }) })
  const [selected, setSelected] = useState<DepositRow | null>(null)
  return <RequireStaff roles={['finance']}><PageHeader title="Refundable bonds" description="Events that have finished and have a completed hall inspection, awaiting a bond decision." />
    <QueryView query={q}>{rows => <Table caption="Bonds awaiting decision" rows={rows} rowKey={r => r.invoice_line_id} columns={[
      { key: 'case', header: 'Request', cell: r => <ButtonLink variant="ghost" to={`/staff/cases/${r.case_id}?tab=finance.money`}>{r.case_number} · {r.applicant_name}</ButtonLink> },
      { key: 'end', header: 'Event finished', cell: r => <DateTime value={r.end_at} /> },
      { key: 'paid', header: 'Bond received', cell: r => <Money cents={r.paid_cents} /> },
      { key: 'action', header: 'Decision', cell: r => <Button onClick={() => setSelected(r)}>Record bond decision</Button> },
    ]} empty={<EmptyState title="No bonds awaiting decision" description="Bonds appear after the event and hall inspection finish." />} />}</QueryView>
    <Dialog title="Refundable bond decision" open={!!selected} onClose={() => setSelected(null)}>{selected ? <DepositForm key={selected.invoice_line_id} caseId={selected.case_id} lineId={selected.invoice_line_id} paid={selected.paid_cents} revision={selected.case_revision} /> : null}</Dialog>
  </RequireStaff>
}
