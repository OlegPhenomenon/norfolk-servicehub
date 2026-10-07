import { Link } from 'react-router'
import { ROLE_LABELS } from '@/api/types'
import { useMe } from '@/auth/useMe'
import type { NavItem } from '@/featureTypes'
import { adminNav, staffNav, visibleNav } from '@/registry'
import { ButtonLink, Card, EmptyState, Icon, PageHeader } from '@/ui'

/*
 * Placeholder overview pages for /my, /staff and /admin. A feature slice replaces one by exporting an
 * `index: true` route in its residentRoutes / staffRoutes / adminRoutes.
 */

function greeting(): string {
  const hour = Number(new Intl.DateTimeFormat('en-AU', { hour: 'numeric', hour12: false, timeZone: 'Pacific/Norfolk' }).format(new Date()))
  return hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening'
}

function NavTiles({ items }: { items: NavItem[] }) {
  return (
    <ul className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
      {items.map((item) => (
        <li key={item.to}>
          <Link to={item.to} className="group flex h-full items-center gap-4 rounded-[var(--radius-card)] border border-line bg-surface p-5 shadow-[var(--shadow-card)] hover:border-primary-200">
            <span className="flex size-11 items-center justify-center rounded-xl bg-primary-50 text-primary">
              <Icon name={item.icon ?? 'chevronRight'} size={22} />
            </span>
            <span className="font-semibold">{item.label}</span>
            <Icon name="arrowRight" size={18} className="ml-auto text-primary transition group-hover:translate-x-1" />
          </Link>
        </li>
      ))}
    </ul>
  )
}

/** `/my` overview (until the services slice provides its own). */
export function ResidentHomePage() {
  const { data: me } = useMe()
  const firstName = me?.user?.name.split(' ')[0] ?? ''
  return (
    <>
      <PageHeader eyebrow="My requests" title={`${greeting()}, ${firstName}`} description="Your requests, messages from the council, payments and results." actions={<ButtonLink to="/services" icon="plus">Start a new request</ButtonLink>} />
      <EmptyState icon="folder" title="No requests yet" description="When you ask the council for a service, it appears here with its progress and anything you need to do." action={<ButtonLink to="/services" variant="secondary">Browse services</ButtonLink>} />
    </>
  )
}

/** `/staff` overview. */
export function StaffHomePage() {
  const { data: me } = useMe()
  const items = visibleNav(staffNav, me?.roles ?? [])
  const firstName = me?.user?.name.split(' ')[0] ?? ''
  return (
    <>
      <PageHeader
        eyebrow="Staff workspace"
        title={`${greeting()}, ${firstName}`}
        description={me?.roles.length ? `You are working as ${me.roles.map((r) => ROLE_LABELS[r]).join(', ')}.` : 'You have no roles yet. Ask the systems administrator to grant one.'}
      />
      {items.length > 0 ? <NavTiles items={items} /> : <EmptyState icon="clipboard" title="Nothing to work on yet" description="Work queues for your roles appear here." />}
    </>
  )
}

/** `/admin` overview. */
export function AdminHomePage() {
  const { data: me } = useMe()
  const items = visibleNav(adminNav, me?.roles ?? [])
  return (
    <>
      <PageHeader eyebrow="Administration" title="Configuration" description="Services, prices, users, settings and integrations. Administrators do not see case content unless they also hold a case role." />
      {items.length > 0 ? (
        <NavTiles items={items} />
      ) : (
        <Card>
          <p className="text-muted">No configuration sections are available for your roles yet.</p>
        </Card>
      )}
    </>
  )
}
