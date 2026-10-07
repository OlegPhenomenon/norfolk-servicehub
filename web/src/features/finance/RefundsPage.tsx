import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { RequireStaff } from '@/auth/RequireStaff'
import { api } from '@/api/client'
import { Button, ButtonLink, DateTime, Dialog, EmptyState, Money, PageHeader, QueryView, StatusPill, Table } from '@/ui'
import type { Refund } from './types'
import { ActionForm } from './ActionForm'
export function RefundsPage() {
  const q = useQuery({ queryKey: ['finance', 'refunds'], queryFn: () => api.get<Refund[]>('/api/finance/refunds'), refetchInterval: 5000 })
  const [confirm, setConfirm] = useState<Refund | null>(null)
  const [recover, setRecover] = useState<Refund | null>(null)
  return <RequireStaff roles={['finance']}><PageHeader title="Refunds" description="Refunds are completed only after DemoPay confirmation or a confirmed bank transfer. DemoPay amounts ending in 13 cents simulate failure." />
    <QueryView query={q}>{rows => <Table caption="Refunds" rows={rows} rowKey={r => r.id} columns={[
      { key: 'case', header: 'Request', cell: r => <ButtonLink variant="ghost" to={`/staff/cases/${r.case_id}?tab=finance.money`}>{r.case_number} · {r.applicant_name}</ButtonLink> },
      { key: 'amount', header: 'Refund', cell: r => <Money cents={r.amount_cents} /> },
      { key: 'reason', header: 'Reason', cell: r => <>{r.reason}{r.failure_reason ? <p>{r.failure_reason}</p> : null}</> },
      { key: 'status', header: 'Status', cell: r => <><StatusPill status={r.status} label={r.status === 'completed' ? 'Refund completed' : undefined} /><DateTime value={r.completed_at} fallback="Awaiting confirmation" /></> },
      { key: 'action', header: 'Action', cell: r => r.method === 'bank_transfer' && r.status === 'processing' ? <Button onClick={() => setConfirm(r)}>Confirm bank transfer</Button> : r.status === 'failed' ? <Button onClick={() => setRecover(r)}>Arrange bank refund</Button> : r.bank_reference || (r.status === 'processing' ? 'Waiting for DemoPay' : '—') },
    ]} empty={<EmptyState title="No refunds requested" />} />}</QueryView>
    <Dialog title="Confirm refund sent by bank" open={!!confirm} onClose={() => setConfirm(null)}>{confirm ? <ActionForm url={`/api/finance/refunds/${confirm.id}/confirm-bank`} label="Confirm refund completed" body={{ expected_revision: confirm.case_revision }} inputs={[{ key: 'bank_reference', label: 'Bank transfer reference' }]} onDone={() => setConfirm(null)} /> : null}</Dialog>
    <Dialog title="Arrange failed refund by bank" open={!!recover} onClose={() => setRecover(null)}>{recover ? <ActionForm url={`/api/finance/refunds/${recover.id}/retry-bank`} label="Arrange bank transfer" body={{ expected_revision: recover.case_revision }} inputs={[{ key: 'reason', label: 'Reason for this recovery action' }]} onDone={() => setRecover(null)} /> : null}</Dialog>
  </RequireStaff>
}
