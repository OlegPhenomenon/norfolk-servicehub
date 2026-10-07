import { useId, type ComponentProps, type ReactNode } from 'react'
import { controlClasses } from './classes'
import { cn } from './cn'
import { FieldContext, useFieldContext } from './fieldContext'
import { Icon } from './Icon'

export interface FieldProps {
  label: ReactNode
  /** Help text under the label. */
  hint?: ReactNode
  /** Validation message; marks the control `aria-invalid`. Pass `apiError.fields[key]`. */
  error?: string | null
  required?: boolean
  /** Visually hide the label (still read by screen readers). */
  hideLabel?: boolean
  /** Override the generated control id. */
  id?: string
  className?: string
  children: ReactNode
}

/**
 * Labelled form row. Put exactly one control inside; it gets id/aria wiring automatically.
 *
 *   <Field label="Email address" hint="We send updates here." error={errors.email} required>
 *     <TextInput type="email" value={email} onChange={(e) => setEmail(e.target.value)} autoComplete="email" />
 *   </Field>
 */
export function Field({ label, hint, error, required = false, hideLabel, id, className, children }: FieldProps) {
  const autoId = useId()
  const controlId = id ?? `f${autoId}`
  const hintId = hint ? `${controlId}-hint` : undefined
  const errorId = error ? `${controlId}-error` : undefined
  const describedBy = [hintId, errorId].filter(Boolean).join(' ') || undefined
  return (
    <div className={cn('flex flex-col gap-1.5', className)}>
      <label htmlFor={controlId} className={cn('font-semibold text-ink leading-snug', hideLabel && 'sr-only')}>
        {label}
        {required ? null : <span className="ml-1.5 font-normal text-muted text-sm">(optional)</span>}
      </label>
      {hint ? (
        <p id={hintId} className="text-sm text-muted -mt-0.5">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className="flex items-start gap-1.5 text-sm font-medium text-danger">
          <Icon name="alert" size={16} className="mt-0.5" />
          <span>{error}</span>
        </p>
      ) : null}
      <FieldContext.Provider value={{ id: controlId, describedBy, invalid: !!error, required }}>{children}</FieldContext.Provider>
    </div>
  )
}

/** Wiring from the surrounding <Field>. Uses `aria-required` (not native `required`) so the browser's own validation bubbles don't fight our messages. */
function useControlProps(props: { id?: string; 'aria-describedby'?: string }) {
  const ctx = useFieldContext()
  return {
    id: props.id ?? ctx?.id,
    'aria-describedby': props['aria-describedby'] ?? ctx?.describedBy,
    'aria-invalid': ctx?.invalid || undefined,
    'aria-required': ctx?.required || undefined,
  }
}

/** <TextInput value={v} onChange={(e) => setV(e.target.value)} /> — use inside <Field>. All native input props work. */
export function TextInput({ className, ...rest }: ComponentProps<'input'>) {
  const wiring = useControlProps(rest)
  return <input type="text" {...rest} {...wiring} className={cn(controlClasses, className)} />
}

/** <Textarea rows={5} value={v} onChange={…} /> — use inside <Field>. */
export function Textarea({ className, rows = 4, ...rest }: ComponentProps<'textarea'>) {
  const wiring = useControlProps(rest)
  return <textarea rows={rows} {...rest} {...wiring} className={cn(controlClasses, 'py-2.5 leading-relaxed resize-y', className)} />
}

export interface SelectOption {
  value: string
  label: string
  disabled?: boolean
}

export interface SelectProps extends Omit<ComponentProps<'select'>, 'children'> {
  options: SelectOption[]
  /** Adds a first empty option, e.g. "Choose…". */
  placeholder?: string
}

/** <Select options={[{ value: 'no', label: 'No' }]} placeholder="Choose…" value={v} onChange={(e) => setV(e.target.value)} /> */
export function Select({ options, placeholder, className, ...rest }: SelectProps) {
  const wiring = useControlProps(rest)
  return (
    <div className="relative">
      <select {...rest} {...wiring} className={cn(controlClasses, 'appearance-none pr-10 cursor-pointer', className)}>
        {placeholder !== undefined ? <option value="">{placeholder}</option> : null}
        {options.map((o) => (
          <option key={o.value} value={o.value} disabled={o.disabled}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={18} className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-muted" />
    </div>
  )
}

/** <FileInput accept="application/pdf,image/*" onChange={(e) => setFile(e.target.files?.[0] ?? null)} /> — use inside <Field>. */
export function FileInput({ className, ...rest }: Omit<ComponentProps<'input'>, 'type'>) {
  const wiring = useControlProps(rest)
  return (
    <input
      type="file"
      {...rest}
      {...wiring}
      className={cn(
        'block w-full text-sm text-muted rounded-lg border border-dashed border-line-strong bg-surface p-2',
        'file:mr-3 file:min-h-10 file:rounded-md file:border-0 file:bg-primary-50 file:px-4 file:font-semibold file:text-primary hover:file:bg-primary-100 file:cursor-pointer',
        'aria-invalid:border-danger',
        className,
      )}
    />
  )
}

export interface CheckboxProps extends Omit<ComponentProps<'input'>, 'type'> {
  label: ReactNode
  hint?: ReactNode
  error?: string | null
}

/** Stand-alone labelled checkbox (do not wrap in <Field>): <Checkbox label="I agree" checked={v} onChange={(e) => setV(e.target.checked)} /> */
export function Checkbox({ label, hint, error, id, className, ...rest }: CheckboxProps) {
  const autoId = useId()
  const controlId = id ?? `c${autoId}`
  const hintId = hint ? `${controlId}-hint` : undefined
  const errorId = error ? `${controlId}-error` : undefined
  return (
    <div className={cn('flex flex-col gap-1', className)}>
      <div className="flex items-start gap-3">
        <input
          type="checkbox"
          id={controlId}
          aria-describedby={[hintId, errorId].filter(Boolean).join(' ') || undefined}
          aria-invalid={!!error || undefined}
          className="mt-0.5 size-6 shrink-0 cursor-pointer rounded accent-primary"
          {...rest}
        />
        <label htmlFor={controlId} className="cursor-pointer leading-snug pt-0.5">
          {label}
          {hint ? (
            <span id={hintId} className="mt-0.5 block text-sm text-muted">
              {hint}
            </span>
          ) : null}
        </label>
      </div>
      {error ? (
        <p id={errorId} className="ml-9 text-sm font-medium text-danger">
          {error}
        </p>
      ) : null}
    </div>
  )
}

export interface RadioGroupProps {
  legend: ReactNode
  name: string
  options: Array<SelectOption & { hint?: ReactNode }>
  value: string | null | undefined
  onChange: (value: string) => void
  hint?: ReactNode
  error?: string | null
  required?: boolean
  disabled?: boolean
  /** Lay options out in a row on wider screens. */
  inline?: boolean
}

/** Radio buttons in a fieldset: good for 2–5 choices (prefer over Select). */
export function RadioGroup({ legend, name, options, value, onChange, hint, error, required = false, disabled, inline }: RadioGroupProps) {
  const autoId = useId()
  const hintId = hint ? `r${autoId}-hint` : undefined
  const errorId = error ? `r${autoId}-error` : undefined
  return (
    <fieldset className="flex flex-col gap-1.5" aria-describedby={[hintId, errorId].filter(Boolean).join(' ') || undefined} disabled={disabled}>
      <legend className="font-semibold text-ink leading-snug mb-1.5">
        {legend}
        {required ? null : <span className="ml-1.5 font-normal text-muted text-sm">(optional)</span>}
      </legend>
      {hint ? (
        <p id={hintId} className="text-sm text-muted -mt-1">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} className="text-sm font-medium text-danger">
          {error}
        </p>
      ) : null}
      <div className={cn('flex gap-2', inline ? 'flex-col sm:flex-row sm:flex-wrap sm:gap-6' : 'flex-col')}>
        {options.map((o) => {
          const optId = `r${autoId}-${o.value}`
          return (
            <div key={o.value} className="flex items-start gap-3 min-h-11 py-1">
              <input
                type="radio"
                id={optId}
                name={name}
                value={o.value}
                checked={value === o.value}
                disabled={o.disabled}
                onChange={() => onChange(o.value)}
                className="mt-0.5 size-6 shrink-0 cursor-pointer accent-primary"
              />
              <label htmlFor={optId} className="cursor-pointer leading-snug pt-0.5">
                {o.label}
                {o.hint ? <span className="mt-0.5 block text-sm text-muted">{o.hint}</span> : null}
              </label>
            </div>
          )
        })}
      </div>
    </fieldset>
  )
}
