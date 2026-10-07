// OWNER: records
import type { CasePanel } from '@/featureTypes'

/*
 * Tabs this feature adds to the case page.
 *   { key: 'records.example', label: 'Example', audience: 'staff', applies: (c) => c.module === 'generic', Component: ExamplePanel }
 */
export const casePanels: CasePanel[] = []
