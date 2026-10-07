import { Link } from 'react-router'
import { useMe } from '@/auth/useMe'
import type { NavItem } from '@/featureTypes'
import { adminNav, visibleNav } from '@/registry'
import { Card, Icon, PageHeader } from '@/ui'

/* Configuration landing page for /admin. */

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
