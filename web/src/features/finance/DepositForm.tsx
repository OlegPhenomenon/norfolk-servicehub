import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, isApiError, newIdempotencyKey } from '@/api/client'
import { Button, Field, TextInput, Textarea, Money, ErrorAlert, useToast } from '@/ui'
import { cents } from './forms'
export function DepositForm({ caseId, lineId, paid, revision }: { caseId: number; lineId: number; paid: number; revision?: number }) {
  const [items, setItems] = useState<{ label: string; amount: string }[]>([])
  const [reason, setReason] = useState('')
  const [refund, setRefund] = useState(`${Math.floor(paid / 100)}.${String(paid % 100).padStart(2, '0')}`)
  const [key] = useState(newIdempotencyKey)
  const qc = useQueryClient()
  const toast = useToast()
  const save = useMutation({ mutationFn: () => api.post(`/api/cases/${caseId}/deposit-decision`, { invoice_line_id: lineId, refund_cents: cents(refund, 'refund_cents'), retain_items: items.map(i => ({ label: i.label, cents: cents(i.amount, 'retain_items') })), reason, expected_revision: revision }, { idempotencyKey: key }), onSuccess: async () => { await qc.invalidateQueries({ queryKey: ['finance'] }); toast.success('Bond decision recorded; any refund awaits confirmation') } })
  const errors = isApiError(save.error) ? save.error.fields : {}
  return <form className="flex flex-col gap-4" onSubmit={e => { e.preventDefault(); save.mutate() }} noValidate>
    <p>Bond paid: <Money cents={paid} />. Refund plus retained items must equal this amount.</p>
    <Field label="Amount to refund (AUD)" required error={errors.refund_cents}><TextInput inputMode="decimal" value={refund} onChange={e => setRefund(e.target.value)} /></Field>
    {items.map((item, index) => <fieldset key={index} className="rounded border border-line p-4 flex flex-col gap-3"><legend>Retained item {index + 1}</legend>
      <Field label="Reason for this charge" required error={errors.retain_items}><TextInput value={item.label} onChange={e => setItems(items.map((v, i) => i === index ? { ...v, label: e.target.value } : v))} /></Field>
      <Field label="Amount retained (AUD)" required error={errors.retain_items}><TextInput inputMode="decimal" value={item.amount} onChange={e => setItems(items.map((v, i) => i === index ? { ...v, amount: e.target.value } : v))} /></Field>
      <Button variant="secondary" onClick={() => setItems(items.filter((_, i) => i !== index))}>Remove item {index + 1}</Button>
    </fieldset>)}
    <Button variant="secondary" onClick={() => setItems([...items, { label: '', amount: '' }])}>Add retained item</Button>
    <Field label="Explain the decision to the applicant" required error={errors.reason}><Textarea value={reason} onChange={e => setReason(e.target.value)} /></Field>
    {save.error ? <ErrorAlert error={save.error} /> : null}
    <Button type="submit" loading={save.isPending}>Record decision and reserve refund</Button>
  </form>
}
