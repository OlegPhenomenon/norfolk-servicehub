export interface InvoiceLine { id: number; kind: 'fee' | 'deposit'; description: string; quantity_milli: number; quantity_minutes: number | null; unit_amount_cents: number; amount_cents: number; paid_cents: number; credited_cents: number; outstanding_cents: number }
export interface Invoice { id: number; number: string; kind: string; status: string; pricing_date: string; total_cents: number; outstanding_cents: number; basis_note: string | null; document_id: number | null; document_version_id: number | null; lines: InvoiceLine[] }
export interface Payment { id: number; source: string; amount_cents: number; received_at: string; status: string; credit_cents: number }
export interface Allocation { id: number; payment_id: number; invoice_line_id: number; amount_cents: number; source: string; external_id: string; reversed_at: string | null; reversal_reason: string | null }
export interface Refund { id: number; case_revision: number; case_id: number; case_number?: string; applicant_name?: string; deposit_decision_id: number; amount_cents: number; method: string; status: string; reason: string; failure_reason: string | null; completed_at: string | null; bank_reference: string | null }
export interface RetainItem { label: string; cents: number }
export interface DepositDecision { id: number; invoice_line_id: number; refund_cents: number; retain_cents: number; reason: string; retain_items: RetainItem[]; decided_at: string }
export interface Evidence { id: number; document_version_id?: number; title: string; case_id?: number; case_number?: string; created_at: string }
export interface JournalLine { id: number; at: string; account: string; memo: string; debit_cents: number; credit_cents: number }
export interface TrialBalance { account: string; debit_cents: number; credit_cents: number; balance_cents: number }
export interface Ledger { entries: JournalLine[]; trial_balance: TrialBalance[] }
export interface MoneyData {
  online_payment_enabled: boolean;
  waiver_quotes?: {item_code:string;description:string;amount_cents:number}[]; summary: { invoiced_cents: number; outstanding_cents: number; paid_cents: number; deposits_held_cents: number; refunded_cents: number; settled: boolean }; invoices: Invoice[]; payments: Payment[]; customer_credit_cents: number; deposit_decisions: DepositDecision[]; refunds: Refund[]; checkout_sessions: { id: number; invoice_id: number; status: string }[]; staff: boolean; revision: number; deposit_ready: boolean; allocations?: Allocation[]; evidence?: Evidence[]; ledger?: Ledger; schedule_note: string }
export interface StatementRow { id: number; amount_cents: number; bank_txn_id: string; txn_date: string; payer_name: string | null; description: string; reference: string | null; status: string; suggested_case_id: number | null }
export interface CaseMatch { id: number; revision: number; number: string; applicant_name: string; title: string; invoices: { id: number; number: string; outstanding_cents: number }[] }
export interface DepositRow { case_revision: number; invoice_line_id: number; case_id: number; case_number: string; applicant_name: string; paid_cents: number; amount_cents: number; end_at: string }
export interface Outstanding { id: number; case_id: number; number: string; case_number: string; applicant_name: string; outstanding_cents: number }
export interface Overview { unmatched: StatementRow[]; refunds: Refund[]; deposits: DepositRow[]; outstanding_invoices: Outstanding[]; todays_receipts: (Payment & { case_id: number | null; case_number: string | null })[] }
export interface PriceVersion { id: number; amount_cents: number; effective_from: string; effective_to: string | null }
export interface Price { code: string; name: string; unit: string; kind: string; versions: PriceVersion[] }
