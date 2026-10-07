import { useQuery } from '@tanstack/react-query'
import { RequireStaff } from '@/auth/RequireStaff'
import { api } from '@/api/client'
import { ButtonLink, Card, DateTime, EmptyState, Money, PageHeader, QueryView, Table } from '@/ui'
import type { Ledger, Overview } from './types'
import { LedgerTable } from './LedgerTable'
export function FinancePage() {
  const q = useQuery({ queryKey: ['finance', 'overview'], queryFn: () => api.get<Overview>('/api/finance/overview'), refetchInterval: 30000 })
  const ledger = useQuery({ queryKey: ['finance', 'ledger'], queryFn: () => api.get<Ledger>('/api/finance/ledger') })
  return <RequireStaff roles={['finance']}><PageHeader title="Finance" description="Confirmed money, charges, refundable bonds and transfers needing attention." actions={<ButtonLink to="/staff/finance/statements">Import bank statement</ButtonLink>} />
    <QueryView query={q}>{data => <div className="flex flex-col gap-5">
      <div className="grid gap-4 sm:grid-cols-3">
        <Card title="Unmatched transfers" actions={<ButtonLink to="/staff/finance/unmatched" variant="secondary">Match transfers</ButtonLink>}><p className="text-3xl font-semibold">{data.unmatched.length}</p></Card>
        <Card title="Refunds needing attention" actions={<ButtonLink to="/staff/finance/refunds" variant="secondary">Review refunds</ButtonLink>}><p className="text-3xl font-semibold">{data.refunds.length}</p></Card>
        <Card title="Bonds awaiting decision" actions={<ButtonLink to="/staff/finance/deposits" variant="secondary">Review bonds</ButtonLink>}><p className="text-3xl font-semibold">{data.deposits.length}</p></Card>
      </div>
      <Card title="Outstanding invoices"><Table caption="Outstanding invoices" rows={data.outstanding_invoices} rowKey={r => r.id} columns={[
        { key: 'number', header: 'Invoice', cell: r => <ButtonLink variant="ghost" to={`/staff/cases/${r.case_id}?tab=finance.money`}>{r.number}</ButtonLink> },
        { key: 'applicant', header: 'Applicant', cell: r => r.applicant_name },
        { key: 'due', header: 'Outstanding', cell: r => <Money cents={r.outstanding_cents} /> },
      ]} empty={<EmptyState title="No outstanding invoices" />} /></Card>
      <Card title="Today's receipts (Norfolk Island time)"><Table caption="Today's receipts" rows={data.todays_receipts} rowKey={r => r.id} columns={[
        { key: 'case', header: 'Request', cell: r => r.case_id ? <ButtonLink variant="ghost" to={`/staff/cases/${r.case_id}?tab=finance.money`}>{r.case_number}</ButtonLink> : 'Unmatched transfer' },
        { key: 'source', header: 'Source', cell: r => r.source.replaceAll('_', ' ') },
        { key: 'amount', header: 'Received', cell: r => <Money cents={r.amount_cents} /> },
        { key: 'time', header: 'Time', cell: r => <DateTime value={r.received_at} format="time" /> },
      ]} empty={<EmptyState title="No receipts today" />} /></Card>
      <QueryView query={ledger}>{data => <LedgerTable data={data} />}</QueryView>
    </div>}</QueryView>
  </RequireStaff>
}
