import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, isApiError, newIdempotencyKey } from '@/api/client'
import { Button, Field, TextInput, Select, ErrorAlert, useToast } from '@/ui'
import { cents } from './forms'
interface Input { key: string; label: string; money?: boolean; type?: 'text' | 'date'; initial?: string; options?: {value:string;label:string}[] }
export function ActionForm({ url, label, inputs, body = {}, onDone }: { url: string; label: string; inputs: Input[]; body?: Record<string, unknown>; onDone?: () => void }) {
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(inputs.map(i => [i.key, i.initial ?? ''])))
  const [key] = useState(newIdempotencyKey)
  const qc = useQueryClient()
  const toast = useToast()
  const save = useMutation({ mutationFn: () => api.post(url, { ...body, ...Object.fromEntries(inputs.map(i => [i.key, i.money ? cents(values[i.key] ?? '', i.key) : values[i.key]])) }, { idempotencyKey: key }), onSuccess: async () => { await qc.invalidateQueries({ queryKey: ['finance'] }); toast.success('Saved'); onDone?.() } })
  const errors = isApiError(save.error) ? save.error.fields : {}
  return <form className="flex flex-col gap-4" onSubmit={e => { e.preventDefault(); save.mutate() }} noValidate>
    {inputs.map(i => <Field key={i.key} label={i.label} required error={errors[i.key]}>{i.options ? <Select placeholder="Choose a fee" value={values[i.key]} onChange={e=>setValues({...values,[i.key]:e.target.value})} options={i.options}/> : <TextInput type={i.type ?? 'text'} inputMode={i.money ? 'decimal' : undefined} value={values[i.key]} onChange={e => setValues({ ...values, [i.key]: e.target.value })} />}</Field>)}
    {save.error ? <ErrorAlert error={save.error} /> : null}
    <Button type="submit" loading={save.isPending}>{label}</Button>
  </form>
}
