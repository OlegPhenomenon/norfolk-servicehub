import type { ReactNode } from 'react'
import { cn } from '@/ui'

/** Standard page width and padding. `narrow` for forms and reading pages. */
export function PageContainer({ narrow, className, children }: { narrow?: boolean; className?: string; children: ReactNode }) {
  return <div className={cn('mx-auto w-full px-4 py-8 sm:px-6 sm:py-10', narrow ? 'max-w-3xl' : 'max-w-7xl', className)}>{children}</div>
}
