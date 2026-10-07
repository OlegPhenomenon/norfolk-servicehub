import { useState } from 'react'
import { Link, NavLink, useLocation } from 'react-router'
import { useMe } from '@/auth/useMe'
import { ButtonLink, cn, Icon } from '@/ui'
import { NotificationBell } from './NotificationBell'
import { UserMenu } from './UserMenu'
import { Wordmark } from './Wordmark'
import { publicNav } from '@/registry'

/** Header of the public site and the resident area. */
export function SiteHeader() {
  const { data: me } = useMe()
  const [menuOpen, setMenuOpen] = useState(false)
  const location = useLocation()
  const [lastPath, setLastPath] = useState(location.pathname)
  if (lastPath !== location.pathname) {
    // Close the mobile menu after navigation.
    setLastPath(location.pathname)
    setMenuOpen(false)
  }
  const user = me?.user

  const navLinkClass = ({ isActive }: { isActive: boolean }) =>
    cn('inline-flex min-h-11 items-center rounded-lg px-3 font-medium', isActive ? 'text-primary bg-primary-50' : 'text-ink/85 hover:text-primary hover:bg-primary-50/60')

  return (
    <header className="border-b border-line bg-surface/95 backdrop-blur supports-[backdrop-filter]:bg-surface/85">
      <div className="mx-auto flex max-w-7xl items-center gap-2 px-4 py-2.5 sm:gap-4 sm:px-6">
        <Wordmark />
        <nav aria-label="Main" className="ml-6 hidden items-center gap-1 xl:flex">
          {publicNav.map((l) => (
            <NavLink key={l.to} to={l.to} className={navLinkClass}>
              {l.label}
            </NavLink>
          ))}
          {user ? (
            <NavLink to={user.kind === 'staff' ? '/staff' : '/my'} className={navLinkClass}>
              {user.kind === 'staff' ? 'Staff workspace' : 'My requests'}
            </NavLink>
          ) : null}
        </nav>
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          {user ? (
            <>
              {/* Staff endpoints answer 401 until TOTP is passed, so the bell waits for it. */}
              {me?.mfa_required ? null : <NotificationBell />}
              <UserMenu />
            </>
          ) : (
            <>
              {me?.demo_mode ? (
                <div className="hidden sm:block">
                  <ButtonLink to="/demo" variant="ghost" icon="users">Try a persona</ButtonLink>
                </div>
              ) : null}
              <ButtonLink to="/login" variant="secondary">
                Sign in
              </ButtonLink>
            </>
          )}
          <button
            type="button"
            aria-expanded={menuOpen}
            aria-controls="mobile-nav"
            onClick={() => setMenuOpen((o) => !o)}
            className="flex size-11 items-center justify-center rounded-lg hover:bg-sunken xl:hidden"
          >
            <Icon name={menuOpen ? 'x' : 'menu'} size={22} title="Menu" />
          </button>
        </div>
      </div>
      {menuOpen ? (
        <nav id="mobile-nav" aria-label="Main" className="border-t border-line px-4 py-2 xl:hidden">
          <ul className="flex flex-col">
            {publicNav.map((l) => (
              <li key={l.to}>
                <NavLink to={l.to} className={navLinkClass}>
                  {l.label}
                </NavLink>
              </li>
            ))}
            {user ? (
              <li>
                <NavLink to={user.kind === 'staff' ? '/staff' : '/my'} className={navLinkClass}>
                  {user.kind === 'staff' ? 'Staff workspace' : 'My requests'}
                </NavLink>
              </li>
            ) : null}
            {me?.demo_mode ? (
              <li>
                <Link to="/demo" className="inline-flex min-h-11 items-center rounded-lg px-3 font-medium text-ink/85 hover:bg-primary-50/60">
                  Demo personas
                </Link>
              </li>
            ) : null}
          </ul>
        </nav>
      ) : null}
    </header>
  )
}
