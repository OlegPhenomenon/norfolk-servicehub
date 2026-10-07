import { createContext, useContext } from 'react'

/** Wiring that `<Field>` passes to the control inside it (TextInput, Textarea, Select, FileInput). */
export interface FieldContextValue {
  id: string
  describedBy: string | undefined
  invalid: boolean
  required: boolean
}

export const FieldContext = createContext<FieldContextValue | null>(null)

/** Used by input components to pick up id / aria wiring from the surrounding `<Field>`. */
export function useFieldContext(): FieldContextValue | null {
  return useContext(FieldContext)
}
