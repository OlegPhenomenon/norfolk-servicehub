import type { ReactNode } from 'react'
import type { UseQueryResult } from '@tanstack/react-query'
import { ErrorAlert } from './Alert'
import { LoadingState } from './Spinner'

export interface QueryViewProps<T> {
  query: UseQueryResult<T>
  /** Loading text. */
  loading?: string
  children: (data: T) => ReactNode
}

/**
 * Standard loading / error / data rendering for a `useQuery` result.
 *   const q = useQuery({ queryKey: ['cases'], queryFn: () => api.get<CaseSummary[]>('/api/cases') })
 *   <QueryView query={q} loading="Loading requests…">{(cases) => <CaseTable cases={cases} />}</QueryView>
 */
export function QueryView<T>({ query, loading, children }: QueryViewProps<T>) {
  if (query.isPending) return <LoadingState label={loading} />
  if (query.isError) return <ErrorAlert error={query.error} title="Could not load this" onRetry={() => void query.refetch()} />
  return <>{children(query.data)}</>
}
