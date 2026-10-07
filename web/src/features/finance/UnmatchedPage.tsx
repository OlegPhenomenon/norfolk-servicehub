import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { RequireStaff } from '@/auth/RequireStaff'
import { Alert, Button, ButtonLink, Card, DateTime, Dialog, EmptyState, ErrorAlert, Field, Money, PageHeader, QueryView, Select, TextInput, Table, useToast } from '@/ui'
import type { CaseMatch, Evidence, StatementRow } from './types'
import { ActionForm } from './ActionForm'
function MatchForm({ row, onDone }: { row: StatementRow; onDone: () => void }) {
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState<number | null>(null)
  const [invoice, setInvoice] = useState('')
  const cases = useQuery({ queryKey: ['finance', 'case-search', search], queryFn: () => api.get<CaseMatch[]>('/api/finance/cases', { query: { q: search } }) })
  const target = cases.data?.find(c => c.id === selected)
  const qc = useQueryClient()
  const toast = useToast()
  const save = useMutation({ mutationFn: () => api.post(`/api/finance/statement-rows/${row.id}/match`, { case_id: selected, expected_revision: target?.revision, invoice_id: invoice ? Number(invoice) : undefined }), onSuccess: async () => { await qc.invalidateQueries({ queryKey: ['finance'] }); toast.success('Transfer matched'); onDone() } })
  const errors = isApiError(save.error) ? save.error.fields : {}
  const due = target?.invoices.find(i => String(i.id) === invoice)?.outstanding_cents ?? target?.invoices.reduce((a, i) => a + i.outstanding_cents, 0)
  return <form className="flex flex-col gap-4" onSubmit={e => { e.preventDefault(); save.mutate() }}>
    <p>Received <Money cents={row.amount_cents} /> from {row.payer_name ?? 'Unknown payer'}. Reference: {row.reference || 'No reference'}.</p>
    <Field label="Search by request number or applicant name" required><TextInput value={search} onChange={e => { setSearch(e.target.value); setSelected(null); setInvoice('') }} /></Field>
    <QueryView query={cases}>{results => <Field label="Request" required error={errors.case_id}><Select placeholder="Choose a request" value={selected ?? ''} options={results.map(c => ({ value: String(c.id), label: `${c.number} — ${c.applicant_name} — ${c.title}` }))} onChange={e => { setSelected(Number(e.target.value)); setInvoice('') }} /></Field>}</QueryView>
    {target ? <><Field label="Invoice" error={errors.invoice_id}><Select value={invoice} options={[{ value: '', label: 'All outstanding invoices, fees first' }, ...target.invoices.map(i => ({ value: String(i.id), label: i.number }))]} onChange={e => setInvoice(e.target.value)} /></Field>
      <Alert title={due != null && row.amount_cents < due ? 'Partial payment' : due != null && row.amount_cents > due ? 'Overpayment' : 'Exact balance'}>Outstanding before matching: <Money cents={due ?? 0} />. {due != null && row.amount_cents < due ? <>Remaining due: <Money cents={due - row.amount_cents} />.</> : due != null && row.amount_cents > due ? <>Surplus customer credit: <Money cents={row.amount_cents - due} />.</> : 'The charges will be settled.'}</Alert>
    </> : null}
    {save.error ? <ErrorAlert error={save.error} /> : null}
    <Button type="submit" disabled={!target} loading={save.isPending}>Confirm match</Button>
  </form>
}
export function UnmatchedPage() {
  const q = useQuery({ queryKey: ['finance', 'unmatched'], queryFn: () => api.get<{ rows: StatementRow[]; evidence: Evidence[]; evidence_note: string }>('/api/finance/unmatched') })
  const [match, setMatch] = useState<StatementRow | null>(null)
  const [ignore, setIgnore] = useState<StatementRow | null>(null)
  return <RequireStaff roles={['finance']}><PageHeader title="Unmatched bank transfers" description="Select the correct request after checking the payer, reference and receipt evidence. Ambiguous references are never matched automatically." />
    <QueryView query={q}>{data => <div className="flex flex-col gap-5"><Table caption="Unmatched transfers" rows={data.rows} rowKey={r => r.id} columns={[
      { key: 'date', header: 'Date', cell: r => <DateTime value={r.txn_date} format="date" /> },
      { key: 'payer', header: 'Payer and reference', cell: r => <><p>{r.payer_name || 'Unknown payer'}</p><p>{r.reference || 'No reference'}</p><p className="text-sm text-muted">{r.description} · {r.bank_txn_id}</p>{r.suggested_case_id ? <ButtonLink variant="ghost" to={`/staff/cases/${r.suggested_case_id}?tab=finance.money`}>View suggested request</ButtonLink> : null}</> },
      { key: 'amount', header: 'Amount', cell: r => <Money cents={r.amount_cents} /> },
      { key: 'actions', header: 'Resolve', cell: r => <div className="flex gap-2 flex-wrap"><Button onClick={() => setMatch(r)}>Match</Button><Button variant="secondary" onClick={() => setIgnore(r)}>Ignore with note</Button></div> },
    ]} empty={<EmptyState title="No unmatched transfers" description="No transfers need matching. Import a bank statement to check new receipts." />} />
      <Card title="Receipt evidence" description={data.evidence_note}>{data.evidence.length ? data.evidence.map(e => <p key={e.id}><ButtonLink variant="ghost" to={`/staff/cases/${e.case_id}?tab=finance.money`}>{e.case_number}: {e.title}</ButtonLink> · Evidence only — not money</p>) : <EmptyState title="No proof of payment uploaded" />}</Card>
    </div>}</QueryView>
    <Dialog open={!!match} onClose={() => setMatch(null)} title="Match bank transfer">{match ? <MatchForm key={match.id} row={match} onDone={() => setMatch(null)} /> : null}</Dialog>
    <Dialog open={!!ignore} onClose={() => setIgnore(null)} title="Ignore statement row">{ignore ? <><p className="mb-4">The received money remains in unallocated receipts.</p><ActionForm url={`/api/finance/statement-rows/${ignore.id}/ignore`} label="Ignore row" inputs={[{ key: 'note', label: 'Reason for ignoring this row' }]} onDone={() => setIgnore(null)} /></> : null}</Dialog>
  </RequireStaff>
}
