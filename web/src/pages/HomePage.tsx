import { Link } from 'react-router'
import { useMe } from '@/auth/useMe'
import { REPO_URL } from '@/config'
import { PineGlyph } from '@/layout/Wordmark'
import { Badge, ButtonLink, cn, Icon, Steps, type IconName } from '@/ui'

interface Story {
  title: string
  area: string
  icon: IconName
  people: Array<{ name: string; staff?: boolean }>
  text: string
}

const STORIES: Story[] = [
  {
    title: 'Alexey hires Rawson Hall',
    area: 'Venues',
    icon: 'calendar',
    people: [{ name: 'Alexey' }, { name: 'Olga', staff: true }, { name: 'Tom', staff: true }, { name: 'Jake', staff: true }],
    text: 'He picks a date and rooms, Olga checks the request, he pays the hire fee and bond online, Jake prepares and inspects the hall and Tom settles the bond. A reschedule or a retained part of the bond keeps its reason in the history.',
  },
  {
    title: 'Building approval with a revised drawing',
    area: 'Planning & Building',
    icon: 'building',
    people: [{ name: 'Alexey' }, { name: 'Priya', staff: true }],
    text: 'Priya comments on the site plan. Alexey uploads a revised drawing — the original stays on file — and Priya, who holds decision authority, issues the approval as a signed PDF.',
  },
  {
    title: 'A planning certificate',
    area: 'Planning & Building',
    icon: 'file',
    people: [{ name: 'Alexey' }, { name: 'Priya', staff: true }],
    text: 'Alexey orders a certificate for his portion. The fee is invoiced on submission, Priya prepares the certificate from recorded sources, and the exact issued version is kept with the request.',
  },
  {
    title: 'Equipment hire billed on actual hours',
    area: 'Works Depot',
    icon: 'truck',
    people: [{ name: 'Alexey' }, { name: 'Jake', staff: true }, { name: 'Tom', staff: true }],
    text: 'Alexey asks for a mini excavator for four hours. Jake records the time actually used, Tom approves it, and the final invoice shows exactly how the amount was calculated.',
  },
  {
    title: 'A road issue, reported and answered',
    area: 'Works & Roads',
    icon: 'pin',
    people: [{ name: 'Alexey' }, { name: 'Olga', staff: true }, { name: 'Helen', staff: true }, { name: 'Jake', staff: true }],
    text: 'Alexey marks a pothole on the map and adds a photo. Olga routes it to the Works Depot, Helen assigns the job, Jake records the repair — and Alexey gets a real answer, not a forwarded email.',
  },
  {
    title: 'A confidential complaint',
    area: 'Feedback',
    icon: 'shield',
    people: [{ name: 'Alexey' }, { name: 'Ruth', staff: true }],
    text: 'A complaint about a staff member goes to Ruth, never to the person it concerns. It is not shown on any public map, and a later review stays linked to the original complaint.',
  },
  {
    title: 'A new service, created by an administrator',
    area: 'Configuration',
    icon: 'settings',
    people: [{ name: 'Mark', staff: true }],
    text: 'Mark turns an existing council paper form into an online service — questions, documents, workflow steps, deadlines and fees — and publishes it without a developer.',
  },
]

const ENTRY_TILES: Array<{ to: string; title: string; text: string; icon: IconName }> = [
  { to: '/services', title: 'Find a service', text: 'Hire a hall or equipment, apply for an approval, order a certificate, report a problem.', icon: 'search' },
  { to: '/my', title: 'My requests', text: 'See where each request is, answer questions, pay and download results.', icon: 'folder' },
  { to: '/staff', title: 'Staff workspace', text: 'Council staff: check, assign, decide and complete requests in one place.', icon: 'clipboard' },
]

/** `/` — public landing page. */
export function HomePage() {
  const { data: me } = useMe()
  return (
    <>
      <section className="relative overflow-hidden border-b border-line bg-gradient-to-b from-primary-50 via-canvas to-canvas">
        <PineGlyph size={420} className="pointer-events-none absolute -top-10 -right-24 hidden text-pine/[0.06] lg:block" />
        <div className="relative mx-auto grid max-w-7xl gap-12 px-4 py-14 sm:px-6 sm:py-20 lg:grid-cols-[1.15fr_1fr] lg:items-center">
          <div>
            <p className="mb-4 inline-flex items-center gap-2 rounded-full border border-pine/20 bg-surface px-3 py-1 text-sm font-medium text-pine">
              <PineGlyph size={16} /> Norfolk Island council services, online
            </p>
            <h1 className="font-serif text-[2.4rem] leading-[1.1] font-semibold tracking-[-0.02em] text-ink sm:text-5xl lg:text-[3.5rem]">
              Ask the council for a service — and see it through to the result
            </h1>
            <p className="mt-5 max-w-xl text-lg text-muted sm:text-xl">
              One place to request, pay and follow every service: a hall booking, an approval, a certificate, a repair. You always know who has your request and what happens next.
            </p>
            <div className="mt-8 flex flex-wrap gap-3">
              <ButtonLink to="/services" size="lg" icon="search">
                Find a service
              </ButtonLink>
              {me?.demo_mode ? (
                <ButtonLink to="/demo" size="lg" variant="secondary" icon="users">
                  Try the demo as a persona
                </ButtonLink>
              ) : (
                <ButtonLink to={me?.user ? '/my' : '/login'} size="lg" variant="secondary">
                  {me?.user ? 'My requests' : 'Sign in'}
                </ButtonLink>
              )}
            </div>
          </div>
          <HeroPreview />
        </div>
      </section>

      <section aria-label="Where to start" className="mx-auto max-w-7xl px-4 py-12 sm:px-6">
        <ul className="grid gap-4 md:grid-cols-3">
          {ENTRY_TILES.map((tile) => (
            <li key={tile.to}>
              <Link
                to={tile.to}
                className="group flex h-full flex-col rounded-[var(--radius-card)] border border-line bg-surface p-6 shadow-[var(--shadow-card)] transition hover:-translate-y-0.5 hover:border-primary-200 hover:shadow-[var(--shadow-raised)]"
              >
                <span className="flex size-12 items-center justify-center rounded-xl bg-primary-50 text-primary transition group-hover:bg-primary group-hover:text-white">
                  <Icon name={tile.icon} size={24} />
                </span>
                <span className="mt-4 flex items-center gap-2 text-xl font-semibold">
                  {tile.title}
                  <Icon name="arrowRight" size={18} className="text-primary transition group-hover:translate-x-1" />
                </span>
                <span className="mt-1.5 text-muted">{tile.text}</span>
              </Link>
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="how-demo" className="border-y border-line bg-surface">
        <div className="mx-auto max-w-7xl px-4 py-14 sm:px-6 sm:py-20">
          <div className="max-w-2xl">
            <p className="text-sm font-semibold tracking-wide text-pine uppercase">How this demo works</p>
            <h2 id="how-demo" className="mt-2 font-serif text-3xl font-semibold tracking-[-0.01em] sm:text-4xl">
              Seven stories you can walk through yourself
            </h2>
            <p className="mt-4 text-lg text-muted">
              Sign in as a resident, make a request, then switch to the staff member who handles it. Every result — documents, invoices, refunds, the dashboard — comes from those actions, not from pre-drawn screens.
            </p>
          </div>
          <ol className="mt-10 grid gap-5 md:grid-cols-2 xl:grid-cols-3">
            {STORIES.map((s, i) => (
              <li key={s.title} className="flex flex-col rounded-[var(--radius-card)] border border-line bg-canvas/60 p-6">
                <div className="flex items-center gap-3">
                  <span className="flex size-10 items-center justify-center rounded-lg bg-surface text-primary ring-1 ring-line">
                    <Icon name={s.icon} size={20} />
                  </span>
                  <span className="text-sm font-medium text-muted">
                    <span className="sr-only">Story </span>
                    {i + 1} · {s.area}
                  </span>
                </div>
                <h3 className="mt-4 text-lg font-semibold leading-snug">{s.title}</h3>
                <p className="mt-2 flex-1 text-[0.95rem] text-ink/85">{s.text}</p>
                <p className="mt-4 flex flex-wrap gap-1.5" aria-label="People in this story">
                  {s.people.map((p) => (
                    <Badge key={p.name} tone={p.staff ? 'accent' : 'primary'}>
                      {p.name}
                    </Badge>
                  ))}
                </p>
              </li>
            ))}
            <li className="flex flex-col justify-between rounded-[var(--radius-card)] bg-pine-900 p-6 text-white">
              <div>
                <PineGlyph size={30} className="text-pine-300" />
                <h3 className="mt-4 font-serif text-2xl font-semibold">Your turn</h3>
                <p className="mt-2 text-white/80">Pick a persona and start with Alexey’s hall booking. Staff sign in with a code from the on-screen Demo authenticator.</p>
              </div>
              <div className="mt-6 flex flex-col items-start gap-3">
                <ButtonLink to="/demo" icon="users" variant="secondary">
                  Choose a persona
                </ButtonLink>
                <Link to="/mock/mail" className="text-sm text-white/85 underline decoration-white/40 underline-offset-2 hover:decoration-white">
                  See the emails the demo would have sent
                </Link>
              </div>
            </li>
          </ol>
        </div>
      </section>

      <section aria-labelledby="owned" className="mx-auto max-w-7xl px-4 py-14 sm:px-6">
        <div className="grid gap-8 rounded-2xl bg-primary-800 p-8 text-white sm:p-10 md:grid-cols-[1fr_auto] md:items-center">
          <div>
            <h2 id="owned" className="font-serif text-2xl font-semibold sm:text-3xl">
              Open source, owned by whoever runs it
            </h2>
            <p className="mt-3 max-w-2xl text-white/80">
              The council can run its own copy — no subscription, no timer, no hidden developer access. Payment, email and records-system connections are simulated here, but the technical cycle around them is real.
            </p>
          </div>
          <a href={REPO_URL} rel="noreferrer" className={cn('inline-flex min-h-12 items-center gap-2 rounded-xl bg-white px-5 font-semibold text-primary-800 hover:bg-primary-50')}>
            View the source <Icon name="external" size={18} />
          </a>
        </div>
      </section>
    </>
  )
}

/** Decorative preview of a request in progress (static markup, not live data). */
function HeroPreview() {
  return (
    <div aria-hidden="true" className="relative mx-auto w-full max-w-md lg:max-w-none">
      <div className="rotate-[0.6deg] rounded-2xl border border-line bg-surface p-6 shadow-[var(--shadow-raised)]">
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold tracking-wide text-pine uppercase">NSH-2026-000142</p>
            <p className="mt-1 text-lg font-semibold">Hire of Rawson Hall</p>
            <p className="text-sm text-muted">Sat 14 Nov · Main hall · 6:00–11:00 pm</p>
          </div>
          <Badge tone="primary" dot>
            In progress
          </Badge>
        </div>
        <div className="mt-5 rounded-xl border border-warning-line bg-warning-50 px-4 py-3 text-sm">
          <p className="font-semibold">Payment received — confirming your booking.</p>
          <p className="text-ink/75">Olga Novak · Customer Care</p>
        </div>
        <Steps
          className="mt-6"
          label="Example progress"
          current="confirm"
          steps={[
            { key: 'check', label: 'Request checked' },
            { key: 'pay', label: 'Hire fee and bond paid' },
            { key: 'confirm', label: 'Confirming your booking' },
            { key: 'event', label: 'Event and hall inspection' },
            { key: 'bond', label: 'Bond returned' },
          ]}
        />
      </div>
    </div>
  )
}
