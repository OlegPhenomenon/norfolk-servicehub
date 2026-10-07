import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Alert, Button, ErrorAlert, Field, QueryView, Select, TextInput, useToast } from '@/ui'
import type { CaseMatch } from './types'
export function CreditForm({ paymentId, caseId, onDone }: { paymentId: number; caseId: number; onDone: () => void }) {
  const [search, setSearch] = useState('')
  const [selected, setSelected] = useState(caseId)
  const [invoice, setInvoice] = useState('')
  const cases = useQuery({ queryKey: ['finance', 'case-search', search], queryFn: () => api.get<CaseMatch[]>('/api/finance/cases', { query: { q: search } }) })
  const target = cases.data?.find(c => c.id === selected)
  const qc = useQueryClient()
  const toast = useToast()
  const save = useMutation({ mutationFn: () => api.post(`/api/finance/payments/${paymentId}/allocate`, { case_id: selected, invoice_id: invoice ? Number(invoice) : undefined, expected_revision: target?.revision }), onSuccess: async () => { await qc.invalidateQueries({ queryKey: ['finance'] }); toast.success('Available credit applied'); onDone() } })
  const errors = isApiError(save.error) ? save.error.fields : {}
  return <form className="flex flex-col gap-4" onSubmit={e => { e.preventDefault(); save.mutate() }}>
    <Alert title="Apply available customer credit">Credit can be applied to this request or another request belonging to the same applicant. The server checks ownership and preserves allocation history.</Alert>
    <Field label="Search by request number or applicant name"><TextInput value={search} onChange={e => { setSearch(e.target.value); setSelected(0); setInvoice('') }} /></Field>
    <QueryView query={cases}>{rows => <Field label="Request to pay" required error={errors.case_id}><Select value={selected || ''} placeholder="Choose a request" options={rows.map(c => ({ value: String(c.id), label: `${c.number} — ${c.applicant_name}` }))} onChange={e => { setSelected(Number(e.target.value)); setInvoice('') }} /></Field>}</QueryView>
    {target ? <Field label="Invoice" error={errors.invoice_id}><Select value={invoice} options={[{ value: '', label: 'All outstanding invoices, fees first' }, ...target.invoices.map(i => ({ value: String(i.id), label: i.number }))]} onChange={e => setInvoice(e.target.value)} /></Field> : null}
    {save.error ? <ErrorAlert error={save.error} /> : null}
    <Button type="submit" disabled={!target} loading={save.isPending}>Apply available credit</Button>
  </form>
}
