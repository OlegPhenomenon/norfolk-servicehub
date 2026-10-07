import { NavLink, Outlet } from 'react-router'
import { RequireAuth } from '@/auth/RequireAuth'
import { useMe } from '@/auth/useMe'
import { residentNav } from '@/registry'
import { cn, Icon } from '@/ui'
import { DemoBanner } from './DemoBanner'
import { SiteFooter } from './SiteFooter'
import { SiteHeader } from './SiteHeader'
import { SkipLink } from './SkipLink'

/** `/my` — signed-in residents and businesses. Sub-navigation = Overview + `residentNav` from features. */
export function ResidentLayout() {
  return (
    <RequireAuth>
      <div className="flex min-h-dvh flex-col">
        <SkipLink />
        <DemoBanner />
        <SiteHeader />
        <ResidentSubnav />
        <main id="main" tabIndex={-1} className="mx-auto w-full max-w-7xl flex-1 px-4 py-8 focus:outline-none sm:px-6 sm:py-10">
          <Outlet />
        </main>
        <SiteFooter />
      </div>
    </RequireAuth>
  )
}

function ResidentSubnav() {
  const { data: me } = useMe()
  const items = [{ to: '/my', label: 'Overview', icon: 'home' as const, end: true }, ...residentNav]
  return (
    <div className="border-b border-line bg-surface">
      <nav aria-label="Your account" className="mx-auto flex max-w-7xl items-center gap-1 overflow-x-auto px-4 sm:px-6">
        <p className="mr-4 hidden shrink-0 py-3 text-sm text-muted lg:block">
          Signed in as <strong className="font-semibold text-ink">{me?.user?.name}</strong>
        </p>
        {items.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            className={({ isActive }) =>
              cn(
                '-mb-px inline-flex min-h-12 shrink-0 items-center gap-2 border-b-[3px] px-3 font-medium whitespace-nowrap',
                isActive ? 'border-primary text-primary' : 'border-transparent text-muted hover:text-ink',
              )
            }
          >
            {item.icon ? <Icon name={item.icon} size={18} /> : null}
            {item.label}
          </NavLink>
        ))}
      </nav>
    </div>
  )
}
