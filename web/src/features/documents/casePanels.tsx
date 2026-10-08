import type { CasePanel } from '@/featureTypes'
import { DocumentsPanel } from './DocumentsPanel'
import { DecisionsPanel } from './DecisionsPanel'
import { BuildingRoutePanel } from './BuildingRoutePanel'
export const casePanels: CasePanel[] = [
  { key: 'documents.documents', label: 'Documents', audience: 'both', applies: () => true, Component: DocumentsPanel },
  { key: 'documents.route', label: 'Fees, scope and exhibition', audience: 'both', applies: c => c.module === 'building', Component: BuildingRoutePanel },
  { key: 'documents.decisions', label: 'Decisions', audience: 'both', applies: () => true, Component: DecisionsPanel },
]
