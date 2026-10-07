import { cn } from './cn'

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'danger-outline' | 'accent'
export type ButtonSize = 'md' | 'sm' | 'lg'

const VARIANTS: Record<ButtonVariant, string> = {
  primary: 'bg-primary text-white shadow-sm hover:bg-primary-700 active:bg-primary-800',
  accent: 'bg-pine text-white shadow-sm hover:bg-pine-700 active:bg-pine-900',
  secondary: 'bg-surface text-primary border border-line-strong shadow-sm hover:bg-primary-50 hover:border-primary-200',
  ghost: 'text-primary hover:bg-primary-50',
  'danger-outline': 'bg-surface text-danger border border-danger/50 hover:bg-danger-50 hover:border-danger',
  danger: 'bg-danger text-white shadow-sm hover:brightness-95 active:brightness-90',
}

const SIZES: Record<ButtonSize, string> = {
  // md and lg meet the 44px touch target; sm is for dense tables/toolbars only.
  sm: 'min-h-9 px-3 text-sm gap-1.5 rounded-lg',
  md: 'min-h-11 px-4 text-[0.95rem] gap-2 rounded-lg',
  lg: 'min-h-13 px-6 text-base gap-2.5 rounded-xl',
}

/** Classes for anything that should look like a button (e.g. a plain `<a>` to a file download). */
export function buttonClasses(variant: ButtonVariant = 'primary', size: ButtonSize = 'md', fullWidth = false): string {
  return cn(
    'inline-flex items-center justify-center font-semibold leading-tight whitespace-nowrap select-none transition-colors',
    'disabled:opacity-55 disabled:cursor-not-allowed aria-disabled:opacity-55 aria-disabled:cursor-not-allowed',
    VARIANTS[variant],
    SIZES[size],
    fullWidth && 'w-full',
  )
}

/** Shared look of text-like controls. */
export const controlClasses =
  'block w-full min-h-11 rounded-lg border border-line-strong bg-surface px-3.5 py-2 text-base text-ink shadow-[inset_0_1px_2px_rgb(23_33_43/0.05)] ' +
  'placeholder:text-subtle hover:border-ink/40 focus-visible:border-primary disabled:bg-sunken disabled:text-muted disabled:cursor-not-allowed ' +
  'aria-invalid:border-danger aria-invalid:bg-danger-50/40'

