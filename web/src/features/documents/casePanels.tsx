import type { CasePanel } from '@/featureTypes'
import { DocumentsPanel } from './DocumentsPanel'
import { DecisionsPanel } from './DecisionsPanel'
import { BuildingRoutePanel } from './BuildingRoutePanel'
import { LettersPanel } from './LettersPanel'
export const casePanels: CasePanel[] = [
  { key: 'documents.documents', label: 'Documents', audience: 'both', applies: () => true, Component: DocumentsPanel },
  { key: 'documents.route', label: 'Fees, scope and exhibition', audience: 'both', applies: c => c.module === 'building', Component: BuildingRoutePanel },
  { key: 'documents.decisions', label: 'Decisions', audience: 'both', applies: () => true, Component: DecisionsPanel },
  {
    key: 'documents.letters', label: 'Response letters', audience: 'staff',
    // Road responses have their own panel (`operations.road-response`); every other response-letter step is issued here.
    applies: (_c, definition) => definition.workflow.steps.some(s => s.handler?.startsWith('documents.letter_issued:') && s.handler !== 'documents.letter_issued:road_response'), Component: LettersPanel },
]
