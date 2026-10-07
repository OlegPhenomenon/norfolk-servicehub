import type { CasePanel } from '@/featureTypes'
import { DocumentsPanel } from './DocumentsPanel'
import { DecisionsPanel } from './DecisionsPanel'
export const casePanels: CasePanel[] = [
  { key: 'documents.documents', label: 'Documents', audience: 'both', applies: () => true, Component: DocumentsPanel },
  { key: 'documents.decisions', label: 'Decisions', audience: 'both', applies: c => ['building', 'planning_certificate'].includes(c.module), Component: DecisionsPanel },
]
