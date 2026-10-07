import { Outlet } from 'react-router'
import { DemoBanner } from './DemoBanner'
import { SiteFooter } from './SiteFooter'
import { SiteHeader } from './SiteHeader'
import { SkipLink } from './SkipLink'

/**
 * Public site shell (`/`, `/services`, `/demo`, `/login`, …). Pages render their own width container;
 * use `<PageContainer>` for the standard one.
 */
export function PublicLayout() {
  return (
    <div className="flex min-h-dvh flex-col">
      <SkipLink />
      <DemoBanner />
      <SiteHeader />
      <main id="main" tabIndex={-1} className="flex-1 focus:outline-none">
        <Outlet />
      </main>
      <SiteFooter />
    </div>
  )
}
