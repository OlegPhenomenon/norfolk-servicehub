import { Link } from 'react-router'
import { REPO_URL } from '@/config'
import { Icon } from '@/ui'
import { PineGlyph } from './Wordmark'

/** Footer with the "fictional demonstration" note and source link. */
export function SiteFooter() {
  return (
    <footer className="mt-auto border-t border-line bg-sunken/60">
      <div className="mx-auto grid max-w-7xl gap-8 px-4 py-10 sm:px-6 md:grid-cols-[1.4fr_1fr_1fr]">
        <div>
          <p className="flex items-center gap-2 font-semibold">
            <PineGlyph size={22} className="text-pine" />
            <span>
              <span className="font-serif">Norfolk</span> ServiceHub
            </span>
          </p>
          <p className="mt-3 max-w-md text-sm text-muted">
            <strong className="font-semibold text-ink">Fictional demonstration.</strong> This is an independent proposal, not an official service of
            Norfolk Island Regional Council. All people, requests, documents and payments are invented; payment, email and records systems are
            simulated.
          </p>
        </div>
        <div>
          <h2 className="text-sm font-semibold tracking-wide text-muted uppercase">Explore</h2>
          <ul className="mt-3 space-y-2 text-sm">
            <li>
              <Link to="/services" className="link">
                Find a service
              </Link>
            </li>
            <li>
              <Link to="/demo" className="link">
                Demo personas
              </Link>
            </li>
            <li>
              <Link to="/mock/mail" className="link">
                DemoMail outbox
              </Link>
            </li>
          </ul>
        </div>
        <div>
          <h2 className="text-sm font-semibold tracking-wide text-muted uppercase">Open source</h2>
          <p className="mt-3 text-sm text-muted">Any council can run its own copy — no subscription, no hidden access.</p>
          <a href={REPO_URL} className="link mt-2 inline-flex items-center gap-1.5 text-sm" rel="noreferrer">
            Source code on GitHub <Icon name="external" size={14} />
          </a>
        </div>
      </div>
    </footer>
  )
}
