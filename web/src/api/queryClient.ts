import { QueryCache, QueryClient } from '@tanstack/react-query'
import { isApiError } from './client'
import { ME_QUERY_KEY } from '../auth/useMe'

/**
 * Shared TanStack Query client.
 * - No retries for 4xx responses (they will not get better by retrying).
 * - Any 401 from a non-`/api/me` query refreshes `useMe()`, so guards can redirect to sign-in / TOTP.
 */
export const queryClient: QueryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error, query) => {
      if (isApiError(error) && error.status === 401 && query.queryKey[0] !== ME_QUERY_KEY[0]) {
        void queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY })
      }
    },
  }),
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      refetchOnWindowFocus: false,
      retry: (failureCount, error) => {
        if (isApiError(error) && error.status >= 400 && error.status < 500) return false
        return failureCount < 2
      },
    },
    mutations: { retry: false },
  },
})
