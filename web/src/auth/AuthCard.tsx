import type { ReactNode } from 'react'
import { PineGlyph } from '@/layout/Wordmark'

/** Centered card used by sign-in, registration and 2FA pages. */
export function AuthCard({ title, description, children, aside }: { title: string; description?: ReactNode; children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="mx-auto grid w-full max-w-5xl gap-8 px-4 py-10 sm:px-6 sm:py-16 lg:grid-cols-[minmax(0,28rem)_1fr] lg:items-start">
      <section className="rounded-2xl border border-line bg-surface p-6 shadow-[var(--shadow-card)] sm:p-8">
        <PineGlyph size={34} className="text-pine" />
        <h1 className="mt-4 font-serif text-3xl font-semibold tracking-[-0.01em]">{title}</h1>
        {description ? <p className="mt-2 text-muted">{description}</p> : null}
        <div className="mt-6">{children}</div>
      </section>
      {aside ? <div>{aside}</div> : null}
    </div>
  )
}
