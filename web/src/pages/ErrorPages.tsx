import { isRouteErrorResponse, useRouteError } from 'react-router'
import { isApiError } from '@/api/client'
import { PageContainer } from '@/layout/PageContainer'
import { ButtonLink, EmptyState, ErrorAlert } from '@/ui'

/** Unknown path. */
export function NotFoundPage() {
  return (
    <PageContainer narrow>
      <EmptyState
        icon="search"
        title="We couldn’t find that page"
        description="The link may be out of date, or this part of the demonstration is not built yet."
        action={
          <ButtonLink to="/" icon="home">
            Go to the home page
          </ButtonLink>
        }
      />
    </PageContainer>
  )
}

/** Route `errorElement`: loader/render errors. */
export function RouteErrorPage() {
  const error = useRouteError()
  if ((isRouteErrorResponse(error) && error.status === 404) || isApiError(error, 'not_found')) return <NotFoundPage />
  return (
    <PageContainer narrow>
      <ErrorAlert error={error instanceof Error ? error : new Error('This page failed to load.')} title="This page could not be shown" onRetry={() => window.location.reload()} />
    </PageContainer>
  )
}
