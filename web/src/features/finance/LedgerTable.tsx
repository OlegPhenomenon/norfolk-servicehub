import { Card, DateTime, EmptyState, Money, Table } from '@/ui'
import type { Ledger } from './types'
export function LedgerTable({ data }: { data: Ledger }) {
  return <Card title="Ledger and trial balance">
    <Table caption="Journal entries" rows={data.entries} rowKey={r => `${r.id}-${r.account}`} columns={[
      { key: 'date', header: 'Posted', cell: r => <DateTime value={r.at} /> },
      { key: 'memo', header: 'Entry', cell: r => r.memo },
      { key: 'account', header: 'Account', cell: r => r.account.replaceAll('_', ' ') },
      { key: 'dr', header: 'Debit', cell: r => <Money cents={r.debit_cents} /> },
      { key: 'cr', header: 'Credit', cell: r => <Money cents={r.credit_cents} /> },
    ]} empty={<EmptyState title="No ledger entries yet" />} />
    <Table className="mt-4" caption="Trial balance" rows={data.trial_balance} rowKey={r => r.account} columns={[
      { key: 'account', header: 'Account', cell: r => r.account.replaceAll('_', ' ') },
      { key: 'dr', header: 'Debits', cell: r => <Money cents={r.debit_cents} /> },
      { key: 'cr', header: 'Credits', cell: r => <Money cents={r.credit_cents} /> },
      { key: 'net', header: 'Balance', cell: r => <Money cents={r.balance_cents} /> },
    ]} />
  </Card>
}
