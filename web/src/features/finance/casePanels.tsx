import type { CasePanel } from '@/featureTypes'
import { MoneyPanel } from './MoneyPanel'
export const casePanels: CasePanel[] = [{ key: 'finance.money', label: 'Money', audience: 'both', applies: () => true, Component: MoneyPanel }]
