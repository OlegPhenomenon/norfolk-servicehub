import { useState, type ReactNode } from 'react'
import { Link, NavLink, Outlet, useLocation } from 'react-router'
import { useMe } from '@/auth/useMe'
import type { NavItem } from '@/featureTypes'
import { visibleNav } from '@/registry'
import { cn, Icon } from '@/ui'
import { DemoBanner } from './DemoBanner'
import { NotificationBell } from './NotificationBell'
import { SkipLink } from './SkipLink'
import { UserMenu } from './UserMenu'
import { Wordmark } from './Wordmark'

interface WorkspaceShellProps {
  /** "Staff workspace" / "Administration". */
  areaName: string
  /** Nav items already including the area's overview entry; filtered by the user's roles here. */
  nav: NavItem[]
  /** Extra links at the bottom of the sidebar. */
  footerNav?: NavItem[]
  /** Optional element above the outlet (e.g. area-wide alerts). */
  banner?: ReactNode
}

/**
 * Shared shell of `/staff` and `/admin`: dark top bar, role-filtered sidebar on wide screens,
 * collapsible menu on mobile. Content is rendered via <Outlet />.
 */
export function WorkspaceShell({ areaName, nav, footerNav = [], banner }: WorkspaceShellProps) {
  const { data: me } = useMe()
  const roles = me?.roles ?? []
  const items = visibleNav(nav, roles)
  const bottom = visibleNav(footerNav, roles)
  const location = useLocation()
  const [menuOpen, setMenuOpen] = useState(false)
  const [lastPath, setLastPath] = useState(location.pathname)
  if (lastPath !== location.pathname) {
    setLastPath(location.pathname)
    setMenuOpen(false)
  }
  const current = [...items, ...bottom].filter((i) => (i.end ? location.pathname === i.to : location.pathname.startsWith(i.to))).sort((a, b) => b.to.length - a.to.length)[0]

  return (
    <div className="flex min-h-dvh flex-col bg-canvas">
      <SkipLink />
      <DemoBanner />
      <header className="sticky top-0 z-30 bg-primary-800 text-white shadow-sm">
        <div className="flex items-center gap-3 px-4 py-2 sm:px-6">
          <Wordmark to={nav[0]?.to ?? '/'} subtitle={areaName} inverted />
          <div className="ml-auto flex items-center gap-1">
            <NotificationBell inverted />
            <UserMenu inverted />
          </div>
        </div>
        <div className="border-t border-white/10 lg:hidden">
          <button
            type="button"
            aria-expanded={menuOpen}
            aria-controls="workspace-nav-mobile"
            onClick={() => setMenuOpen((o) => !o)}
            className="flex min-h-12 w-full items-center gap-2 px-4 text-left font-semibold sm:px-6"
          >
            <Icon name="menu" size={20} />
            <span>{current?.label ?? 'Menu'}</span>
            <Icon name="chevronDown" size={18} className={cn('ml-auto transition-transform', menuOpen && 'rotate-180')} />
          </button>
          {menuOpen ? (
            <nav id="workspace-nav-mobile" aria-label={areaName} className="border-t border-white/10 bg-primary-900 px-2 py-2">
              <NavList items={[...items, ...bottom]} inverted />
            </nav>
          ) : null}
        </div>
      </header>
      <div className="flex flex-1">
        <aside className="hidden w-64 shrink-0 border-r border-line bg-surface lg:block">
          <nav aria-label={areaName} className="sticky top-[60px] flex max-h-[calc(100dvh-60px)] flex-col gap-6 overflow-y-auto px-3 py-6">
            <NavList items={items} />
            {bottom.length > 0 ? (
              <div className="border-t border-line pt-4">
                <NavList items={bottom} />
              </div>
            ) : null}
            <p className="mt-auto px-3 text-xs text-subtle">
              <Link to="/" className="hover:text-primary hover:underline">
                ← Public site
              </Link>
            </p>
          </nav>
        </aside>
        <main id="main" tabIndex={-1} className="min-w-0 flex-1 px-4 py-6 focus:outline-none sm:px-6 lg:px-10 lg:py-8">
          <div className="mx-auto max-w-6xl">
            {banner}
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  )
}

function NavList({ items, inverted }: { items: NavItem[]; inverted?: boolean }) {
  return (
    <ul className="flex flex-col gap-0.5">
      {items.map((item) => (
        <li key={item.to}>
          <NavLink
            to={item.to}
            end={item.end}
            className={({ isActive }) =>
              cn(
                'flex min-h-11 items-center gap-3 rounded-lg px-3 font-medium',
                inverted
                  ? isActive
                    ? 'bg-white/15 text-white'
                    : 'text-white/85 hover:bg-white/10 hover:text-white'
                  : isActive
                    ? 'bg-primary-50 text-primary shadow-[inset_3px_0_0_var(--color-primary)]'
                    : 'text-ink/80 hover:bg-sunken hover:text-ink',
              )
            }
          >
            <Icon name={item.icon ?? 'chevronRight'} size={19} className="opacity-90" />
            {item.label}
          </NavLink>
        </li>
      ))}
    </ul>
  )
}
