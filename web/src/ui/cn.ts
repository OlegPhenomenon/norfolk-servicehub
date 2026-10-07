/** Join class names, skipping falsy values: `cn('px-4', active && 'bg-primary')`. */
export function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(' ')
}
