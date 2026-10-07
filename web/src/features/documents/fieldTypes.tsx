import type { FieldComponent } from '@/featureTypes'
import { DecisionPicker } from './DecisionPicker'
export const fieldTypes: Record<string, FieldComponent> = { decision_ref: DecisionPicker }
