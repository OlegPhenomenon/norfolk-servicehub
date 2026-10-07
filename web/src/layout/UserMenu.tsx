import { Link, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { api } from '@/api/client'
import { ROLE_LABELS } from '@/api/types'
import { useMe, useRefreshMe } from '@/auth/useMe'
import { cn, Icon, useToast } from '@/ui'
import { usePopover } from './usePopover'

function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase())
    .join('')
}

/** Signed-in user's avatar button with links to their areas and Sign out. */
export function UserMenu({ inverted }: { inverted?: boolean }) {
  const { data: me } = useMe()
  const refreshMe = useRefreshMe()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const toast = useToast()
  const { open, setOpen, containerRef, triggerRef } = usePopover()
  const user = me?.user
  if (!user || !me) return null

  const signOut = async () => {
    setOpen(false)
    try {
      await api.post('/api/auth/logout')
    } catch (e) {
      toast.error(e)
    }
    qc.removeQueries({ predicate: (q) => q.queryKey[0] !== 'me' })
    // Leave the guarded area first, otherwise its guard redirects to /login when the session disappears.
    await navigate('/')
    await refreshMe()
  }

  const isStaff = user.kind === 'staff'
  const links = [
    { to: '/my', label: 'My requests', icon: 'folder' as const, show: !isStaff },
    { to: '/staff', label: 'Staff workspace', icon: 'clipboard' as const, show: isStaff },
    { to: '/admin', label: 'Administration', icon: 'settings' as const, show: me.roles.includes('sysadmin') || me.roles.includes('manager') },
    { to: '/staff/settings/2fa', label: 'Two-step verification', icon: 'key' as const, show: isStaff },
    { to: '/demo', label: 'Switch persona', icon: 'users' as const, show: me.demo_mode },
  ].filter((l) => l.show)

  return (
    <div ref={containerRef} className="relative">
      <button
        ref={triggerRef}
        type="button"
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => setOpen(!open)}
        className={cn('flex min-h-11 items-center gap-2 rounded-lg py-1 pr-2 pl-1', inverted ? 'text-white hover:bg-white/10' : 'hover:bg-sunken')}
      >
        <span aria-hidden="true" className={cn('flex size-9 items-center justify-center rounded-full text-sm font-bold', isStaff ? 'bg-pine text-white' : 'bg-primary text-white')}>
          {initials(user.name)}
        </span>
        <span className="hidden max-w-40 truncate text-sm font-semibold md:block">{user.name}</span>
        <span className="sr-only md:hidden">Account menu for {user.name}</span>
        <Icon name="chevronDown" size={16} className="hidden md:block" />
      </button>
      {open ? (
        <div className="absolute right-0 z-40 mt-2 w-72 overflow-hidden rounded-xl border border-line bg-surface text-ink shadow-[var(--shadow-raised)]">
          <div className="border-b border-line px-4 py-3">
            <p className="font-semibold">{user.name}</p>
            <p className="truncate text-sm text-muted">{user.job_title ?? user.email}</p>
            {me.roles.length > 0 ? <p className="mt-1 text-xs text-muted">{me.roles.map((r) => ROLE_LABELS[r]).join(' · ')}</p> : null}
          </div>
          <ul className="py-1">
            {links.map((l) => (
              <li key={l.to}>
                <Link to={l.to} onClick={() => setOpen(false)} className="flex min-h-11 items-center gap-3 px-4 hover:bg-primary-50/60">
                  <Icon name={l.icon} size={18} className="text-muted" />
                  {l.label}
                </Link>
              </li>
            ))}
            <li className="border-t border-line">
              <button type="button" onClick={() => void signOut()} className="flex min-h-11 w-full items-center gap-3 px-4 text-left hover:bg-primary-50/60">
                <Icon name="logout" size={18} className="text-muted" />
                Sign out
              </button>
            </li>
          </ul>
        </div>
      ) : null}
    </div>
  )
}
