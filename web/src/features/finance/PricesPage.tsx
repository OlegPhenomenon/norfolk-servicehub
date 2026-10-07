import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import { RequireStaff } from '@/auth/RequireStaff'
import { Alert, Button, Card, DateTime, Dialog, Money, PageHeader, QueryView, StatusPill, norfolkToday } from '@/ui'
import type { Price } from './types'
import { ActionForm } from './ActionForm'
export function PricesPage() {
  const q = useQuery({ queryKey: ['finance', 'prices'], queryFn: () => api.get<{ items: Price[]; schedule_note: string }>('/api/admin/prices') })
  const [selected, setSelected] = useState<Price | null>(null)
  return <RequireStaff roles={['finance', 'sysadmin']}><PageHeader title="Prices" description="Schedule future rates. Issued invoices keep the version and amount used when they were issued." />
    <QueryView query={q}>{data => <div className="flex flex-col gap-5"><Alert title={data.schedule_note}>Rates are a demonstration copy. Hall hire is charged for every calendar day the booking touches. Illustrative amounts are named on each item.</Alert>
      {data.items.map(item => <Card key={item.code} title={item.name} description={`${item.code} · per ${item.unit}`} actions={<Button variant="secondary" onClick={() => setSelected(item)}>Schedule new price from date</Button>}>
        <ol aria-label={`${item.name} price history`} className="flex flex-col gap-4 border-l-2 border-line pl-4">{item.versions.map(v => <li key={v.id} className="flex flex-wrap gap-x-4 gap-y-1"><Money cents={v.amount_cents} /><span>From <DateTime value={v.effective_from} format="date" />{v.effective_to ? <> until <DateTime value={v.effective_to} format="date" /> (exclusive)</> : ' onwards'}</span>{v.effective_from > norfolkToday() ? <StatusPill status="scheduled" /> : null}</li>)}</ol>
      </Card>)}
    </div>}</QueryView>
    <Dialog title={`Schedule price: ${selected?.name ?? ''}`} open={!!selected} onClose={() => setSelected(null)}>{selected ? <ActionForm key={selected.code} url={`/api/admin/prices/${selected.code}/versions`} label="Schedule new price" inputs={[{ key: 'amount_cents', label: 'New amount (AUD)', money: true }, { key: 'effective_from', label: 'Effective from (Norfolk Island date)', type: 'date' }]} onDone={() => setSelected(null)} /> : null}</Dialog>
  </RequireStaff>
}
