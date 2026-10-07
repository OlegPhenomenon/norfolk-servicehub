import type { CasePanel } from '@/featureTypes'
import { ComplaintPanel, RecordsPanel } from './Panels'
export const casePanels: CasePanel[] = [
  { key: 'records.complaint', label: 'Confidential feedback', audience: 'both', applies: c => c.module === 'complaint', Component: ComplaintPanel },
  { key: 'records.records', label: 'Records', audience: 'staff', applies: () => true, Component: RecordsPanel },
]
