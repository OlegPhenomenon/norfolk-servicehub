import { useEffect, useState } from 'react'
import { useLocation } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '@/api/client'
import { hasRole, useMe } from '@/auth/useMe'
import { Alert, Button, Card, DateTime, DescriptionList, Dialog, EmptyState, ErrorAlert, Money, QueryView, StatusPill, Table } from '@/ui'
import type { Allocation, Invoice, MoneyData } from './types'
import { ActionForm } from './ActionForm'
import { DepositForm } from './DepositForm'
import { LedgerTable } from './LedgerTable'
import { CreditForm } from './CreditForm'
export function MoneyPanel({ caseId }: { caseId: number }) {
  const client = useQueryClient()
  const location = useLocation()
  const returned = new URLSearchParams(location.search).get('paid') === '1'
  const me = useMe().data
  const finance = hasRole(me, 'finance')
  const manager = hasRole(me, 'manager')
  const [refundOpen, setRefundOpen] = useState(false), [unmatchId,setUnmatchId] = useState<number|null>(null), [waiverOpen,setWaiverOpen] = useState(false)
  const q = useQuery({ queryKey: ['finance', 'money', caseId], queryFn: () => api.get<MoneyData>(`/api/cases/${caseId}/money`), refetchInterval: query => {
    const data = query.state.data
    const open = data?.checkout_sessions.some(s => s.status === 'open')
    return (returned && !data || open || data?.refunds.some(r => r.status === 'processing')) ? Math.min(1000 * 2 ** Math.min(query.state.dataUpdateCount, 5), 30000) : false
  } })
  // Settlement webhooks can advance or close the case while this panel is polling.
  // Refresh its status and workflow alongside confirmed money, without a page reload.
  const settlement = q.data ? JSON.stringify([q.data.payments.map(p => p.id), q.data.refunds.map(r => [r.id, r.status])]) : null
  useEffect(() => {
    if (settlement !== null) void client.invalidateQueries({ queryKey: ['cases', 'detail', String(caseId)] })
  }, [client, caseId, settlement])
  const [counter, setCounter] = useState<Invoice | null>(null)
  const [reverse, setReverse] = useState<Allocation | null>(null)
  const [deposit, setDeposit] = useState<{ lineId: number; paid: number } | null>(null)
  const [creditPayment, setCreditPayment] = useState<number | null>(null)
  const pay = useMutation({ mutationFn: (invoice_id: number) => api.post<{ checkout_url: string }>(`/api/cases/${caseId}/checkout`, { invoice_id }), onSuccess: result => { window.location.assign(result.checkout_url) } })
  return <>{returned && q.isPending ? <div aria-live="polite"><Alert title="Payment processing">Waiting for confirmation from the payment provider.</Alert></div> : null}<QueryView query={q}>{data => <div className="flex flex-col gap-5">
    <p className="text-sm text-muted">{data.schedule_note}</p>
    <div aria-live="polite" aria-atomic="true">
      {data.checkout_sessions.some(s => s.status === 'open') ? <Alert title="Payment processing">Payment processing — waiting for confirmation from the payment provider.</Alert>
        : data.checkout_sessions[0]?.status === 'failed' ? (data.staff
          ? <Alert tone="warning" title="Applicant's online payment was declined">The invoice is still outstanding.</Alert>
          : <Alert tone="warning" title="Payment declined">Your charges remain unpaid. You can try again.</Alert>)
        : returned && !data.staff && data.checkout_sessions[0]?.status === 'paid' ? <Alert tone="success" title="Payment confirmed">The payment provider has confirmed your payment. Any remaining balance is shown below.</Alert> : null}
    </div>
    <Card title={data.staff ? "Applicant's balance" : 'Your balance'}><DescriptionList columns={2} items={[
      { label: 'Outstanding charges', value: <Money cents={data.summary.outstanding_cents} /> },
      { label: data.staff ? 'Customer credit available' : 'Your credit', value: <Money cents={data.customer_credit_cents} /> },
      { label: 'Refundable bond held', value: <Money cents={data.summary.deposits_held_cents} /> },
      { label: 'Refunds completed', value: <Money cents={data.summary.refunded_cents} /> },
    ]} /></Card>
    {finance && data.staff && data.customer_credit_cents > 0 && <Button onClick={()=>setRefundOpen(true)}>Refund customer credit</Button>}
    {manager && data.staff && !data.invoices.some(i=>i.kind==='invoice') && <Button variant="secondary" onClick={()=>setWaiverOpen(true)}>Approve price exemption</Button>}
    <Dialog title="Refund customer credit" open={refundOpen} onClose={()=>setRefundOpen(false)}><ActionForm url={`/api/cases/${caseId}/refund-credit`} label="Request refund" body={{expected_revision:data.revision}} inputs={[{key:'amount_cents',label:'Refund amount (AUD)',money:true},{key:'reason',label:'Reason for the refund (shown to the applicant)'}]} onDone={()=>setRefundOpen(false)} /></Dialog>
    <Dialog title="Return bank transfer to suspense" open={unmatchId !== null} onClose={()=>setUnmatchId(null)}><ActionForm url={`/api/finance/payments/${unmatchId}/unmatch`} label="Unmatch payment" body={{expected_revision:data.revision}} inputs={[{key:'reason',label:'Reason for correction (staff only)'}]} onDone={()=>setUnmatchId(null)} /></Dialog>
    <Dialog title="Approve price exemption before invoicing" open={waiverOpen} onClose={()=>setWaiverOpen(false)}><ActionForm url={`/api/cases/${caseId}/price-waivers`} label="Approve exemption" body={{expected_revision:data.revision}} inputs={[{key:'item_code',label:'Fee to waive',options:(data.waiver_quotes??[]).map(l=>({value:l.item_code,label:l.description}))},{key:'amount_cents',label:'Amount waived (AUD)',money:true},{key:'reason',label:'Exemption reason'}]} onDone={()=>setWaiverOpen(false)} /></Dialog>
    {pay.error ? <ErrorAlert error={pay.error} /> : null}
    {data.invoices.length === 0 ? <EmptyState title="No charges yet" description="An invoice will appear here when payment is required." /> : data.invoices.map(invoice => <Card key={invoice.id} title={invoice.number} description={`${invoice.kind.replaceAll('_', ' ')} • priced for`} actions={<div className="flex gap-2 flex-wrap"><DateTime value={invoice.pricing_date} format="date" />{invoice.kind === 'invoice' && invoice.outstanding_cents > 0 ? (data.staff
      // Online payment is the applicant's action; finance records money taken at the counter.
      ? <><StatusPill status="awaiting_payment" tone="warning" label="Awaiting payment from applicant" />{finance ? <Button variant="secondary" onClick={() => setCounter(invoice)}>Record counter payment</Button> : null}</>
      : data.online_payment_enabled && <Button loading={pay.isPending} onClick={() => pay.mutate(invoice.id)}>Pay <Money cents={invoice.outstanding_cents} /></Button>)
      : <StatusPill status={invoice.kind === 'invoice' ? 'paid' : invoice.kind} label={invoice.kind === 'invoice' ? 'Settled' : undefined} />}</div>}>
      <Table caption={`${invoice.number} charges`} rows={invoice.lines} rowKey={l => l.id} columns={[
        { key: 'description', header: 'Charge', cell: l => <><p>{l.kind === 'deposit' ? 'Refundable bond' : l.description}</p>{l.kind === 'deposit' ? <p className="text-sm text-muted">Held until inspection and a bond decision</p> : null}<p className="text-sm text-muted">{l.quantity_minutes != null ? `${l.quantity_minutes} ${invoice.kind === 'estimate' ? 'estimated' : 'actual'} minutes` : `${l.quantity_milli / 1000} units`} at <Money cents={l.unit_amount_cents} /></p></> },
        { key: 'amount', header: 'Amount', cell: l => <Money cents={l.amount_cents} /> },
        { key: 'paid', header: 'Received', cell: l => <Money cents={l.paid_cents} /> },
        { key: 'credit', header: 'Credited', cell: l => <Money cents={l.credited_cents} /> },
        { key: 'due', header: 'Due', cell: l => invoice.kind === 'invoice' ? <Money cents={l.outstanding_cents} /> : '—' },
      ]} />
      {invoice.basis_note ? <p className="mt-3 text-sm">{invoice.basis_note}</p> : null}
      {invoice.document_id ? <a className="link inline-block mt-3" href={`/api/document-versions/${invoice.document_version_id}/download`}>Download {invoice.kind.replaceAll('_', ' ')} PDF</a> : null}
      {finance && data.staff && data.deposit_ready ? invoice.lines.filter(l => l.kind === 'deposit' && invoice.kind === 'invoice' && l.paid_cents > 0 && l.outstanding_cents === 0 && !data.deposit_decisions.some(d => d.invoice_line_id === l.id)).map(l => <Button key={l.id} className="mt-4" onClick={() => setDeposit({ lineId: l.id, paid: l.paid_cents })}>Record bond decision</Button>) : null}
    </Card>)}
    <Card title="Payments received"><Table caption="Confirmed payments" rows={data.payments} rowKey={p => p.id} columns={[
      { key: 'date', header: 'Received', cell: p => <DateTime value={p.received_at} /> },
      { key: 'source', header: 'Source', cell: p => <>{p.source === 'provider' ? 'DemoPay' : p.source === 'counter' ? 'Customer Care counter' : 'Bank transfer'}{p.source === 'bank_transfer' && finance && data.staff && <Button variant="secondary" onClick={()=>setUnmatchId(p.id)}>Unmatch to suspense</Button>}</> },
      { key: 'amount', header: 'Amount', cell: p => <Money cents={p.amount_cents} /> },
      { key: 'credit', header: 'Available credit', cell: p => <><Money cents={p.credit_cents} />{finance && data.staff && p.credit_cents > 0 ? <Button variant="secondary" onClick={() => setCreditPayment(p.id)}>Apply credit</Button> : null}</> },
    ]} empty={<EmptyState title="No confirmed payments" description="Uploaded receipts are evidence only and do not count as money received." />} /></Card>
    {data.deposit_decisions.map(d => <Card key={d.id} title="Refundable bond decision"><p>{d.reason}</p><p className="my-2">Refund: <Money cents={d.refund_cents} /> · Retained: <Money cents={d.retain_cents} /></p>{d.retain_items.map((item, i) => <p key={i}>{item.label}: <Money cents={item.cents} /></p>)}<DateTime value={d.decided_at} /></Card>)}
    <Card title="Refunds"><Table caption="Refund status" rows={data.refunds} rowKey={r => r.id} columns={[
      { key: 'amount', header: 'Amount', cell: r => <Money cents={r.amount_cents} /> },
      { key: 'reason', header: 'Reason', cell: r => r.reason },
      { key: 'status', header: 'Status', cell: r => <><StatusPill status={r.status} label={r.status === 'completed' ? 'Refund completed' : r.status === 'failed' ? (data.staff ? 'Refund failed — action required (see Refunds)' : 'Finance is arranging your refund') : 'Refund processing'} /><DateTime value={r.completed_at} fallback="Awaiting confirmation" />{r.failure_reason && data.staff ? <p>{r.failure_reason}</p> : null}</> },
    ]} empty={<EmptyState title="No refunds requested" />} /></Card>
    {data.staff ? <>
      <Card title="Proof of payment" description="Evidence only — not money. Uploaded receipts never create payments.">{data.evidence?.length ? data.evidence.map(e => <p key={e.id}><a className="link" href={`/api/document-versions/${e.document_version_id}/download`}>{e.title}</a> · <DateTime value={e.created_at} /></p>) : <EmptyState title="No receipt evidence uploaded" />}</Card>
      <Card title="Allocation history"><Table caption="Payment allocations" rows={data.allocations ?? []} rowKey={a => a.id} columns={[
        { key: 'source', header: 'Payment', cell: a => `${a.source.replaceAll('_', ' ')} · ${a.external_id}` },
        { key: 'line', header: 'Invoice line', cell: a => `Line ${a.invoice_line_id}` },
        { key: 'amount', header: 'Allocated', cell: a => <Money cents={a.amount_cents} /> },
        { key: 'status', header: 'History', cell: a => a.reversed_at ? <><p>Reversed: {a.reversal_reason}</p><DateTime value={a.reversed_at} /></> : finance ? <Button variant="secondary" onClick={() => setReverse(a)}>Reverse allocation</Button> : 'Active' },
      ]} empty={<EmptyState title="No allocations yet" />} /></Card>
      {data.ledger ? <LedgerTable data={data.ledger} /> : null}
    </> : null}
    <Dialog title="Record cash or EFTPOS received at Customer Care" open={!!counter} onClose={() => setCounter(null)}>{counter ? <ActionForm url="/api/finance/payments/counter" label="Record confirmed payment" body={{ case_id: caseId, invoice_id: counter.id, expected_revision: data.revision }} inputs={[{ key: 'amount_cents', label: 'Amount received (AUD)', money: true }, { key: 'receipt_no', label: 'Unique counter receipt number' }]} onDone={() => setCounter(null)} /> : null}</Dialog>
    <Dialog title="Reverse allocation" open={!!reverse} onClose={() => setReverse(null)}>{reverse ? <ActionForm url={`/api/finance/allocations/${reverse.id}/reverse`} label="Reverse into customer credit" inputs={[{ key: 'reason', label: 'Reason (staff only)' }]} body={{ expected_revision: data.revision }} onDone={() => setReverse(null)} /> : null}</Dialog>
    <Dialog title="Bond decision" open={!!deposit} onClose={() => setDeposit(null)}>{deposit ? <DepositForm caseId={caseId} lineId={deposit.lineId} paid={deposit.paid} revision={data.revision} /> : null}</Dialog>
    <Dialog title="Apply available customer credit" open={creditPayment != null} onClose={() => setCreditPayment(null)}>{creditPayment != null ? <CreditForm paymentId={creditPayment} caseId={caseId} onDone={() => setCreditPayment(null)} /> : null}</Dialog>
  </div>}</QueryView></>
}
