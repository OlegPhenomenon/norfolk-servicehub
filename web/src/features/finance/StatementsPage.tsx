import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { RequireStaff } from '@/auth/RequireStaff'
import { Alert, Button, ButtonLink, Card, Field, FileInput, ErrorAlert, Money, PageHeader, StatusPill, Table } from '@/ui'
import type { StatementRow } from './types'
export function StatementsPage() {
  const [file, setFile] = useState<File | null>(null)
  const qc = useQueryClient()
  const upload = useMutation({ mutationFn: async () => {
    if (!file) throw new Error('Choose a CSV file first.')
    return api.post<{ import_id: number; rows: StatementRow[] }>('/api/finance/statements', { filename: file.name, csv: await file.text() })
  }, onSuccess: async () => { await qc.invalidateQueries({ queryKey: ['finance'] }) } })
  const errors = isApiError(upload.error) ? upload.error.fields : {}
  return <RequireStaff roles={['finance']}><PageHeader title="Import bank statement" description="Each confirmed transfer is counted once. Exact references and balances match automatically." />
    <div className="flex flex-col gap-5"><Card title="CSV file"><p className="mb-4 break-words">Columns: date,amount,description,reference,bank_txn_id,payer. Dates use YYYY-MM-DD; amounts use dollars and cents (for example 181.13).</p>
      <form className="flex flex-col gap-4" onSubmit={e => { e.preventDefault(); upload.mutate() }}>
        <Field label="Bank statement CSV" required error={errors.file ?? errors.amount}><FileInput accept=".csv,text/csv" onChange={e => setFile(e.target.files?.[0] ?? null)} /></Field>
        {upload.error ? <ErrorAlert error={upload.error} /> : null}
        <Button type="submit" loading={upload.isPending} disabled={!file}>Import statement</Button>
      </form>
    </Card>
    <div aria-live="polite">{upload.data ? <><Alert tone="success" title="Statement imported">{upload.data.rows.filter(r => r.status === 'matched').length} matched · {upload.data.rows.filter(r => r.status === 'unmatched').length} unmatched · {upload.data.rows.filter(r => r.status === 'duplicate').length} duplicates. Duplicate rows do not count as new receipts.</Alert>
      <Table caption="Import results" rows={upload.data.rows} rowKey={r => r.id} columns={[
        { key: 'id', header: 'Transaction', cell: r => r.bank_txn_id },
        { key: 'amount', header: 'Amount', cell: r => <Money cents={r.amount_cents} /> },
        { key: 'status', header: 'Result', cell: r => <StatusPill status={r.status} /> },
      ]} /><ButtonLink className="mt-4" to="/staff/finance/unmatched">Review unmatched transfers</ButtonLink>
    </> : null}</div></div>
  </RequireStaff>
}
