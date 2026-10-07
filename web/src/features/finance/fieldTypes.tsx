// OWNER: finance
import type { FieldComponent } from '@/featureTypes'

/*
 * Form widgets this feature provides, keyed by `FieldDef.type` (e.g. `booking_slot`).
 * The widget must emit the canonical answer value for its type (see `AnswerValue` in api/types.ts).
 */
export const fieldTypes: Record<string, FieldComponent> = {}
