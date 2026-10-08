import type { FieldComponent } from '@/featureTypes'
import { DecisionPicker } from './DecisionPicker'
import { ProjectRefPicker } from './ProjectRefPicker'
export const fieldTypes: Record<string, FieldComponent> = { decision_ref: DecisionPicker, project_ref: ProjectRefPicker }
