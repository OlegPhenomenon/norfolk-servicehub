import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api } from '@/api/client'
import { PageContainer } from '@/layout/PageContainer'
import {
  PageHeader,
  QueryView,
  EmptyState,
  Card,
  DateTime,
  StatusPill,
} from '@/ui'
import { IslandMap } from './IslandMap'
import type { Road } from './types'
export function RoadMapPage() {
  const q = useQuery({
    queryKey: ['operations', 'public-roads'],
    queryFn: () => api.get<Road[]>('/api/public/road-issues'),
  })
  return (
    <PageContainer>
      <PageHeader
        title="Road issues map"
        description="Fictional demo reports on Norfolk Island. See where Council is reviewing reported issues and where work has been completed."
        actions={
          <Link className="link" to="/services/road-issue">
            Report a road issue
          </Link>
        }
      />
      <QueryView query={q}>
        {(rows) => (
          <div className="space-y-5">
            <IslandMap
              points={rows.map((r) => ({
                ...r.location,
                label: `${r.category}: ${r.status_text}`,
                completed: r.status_text === 'Work completed',
              }))}
            />
            {rows.length ? (
              <section aria-label="Road issues list">
                <h2 className="mb-3 text-xl font-semibold">
                  Reports (map list)
                </h2>
                {rows.map((r) => (
                  <Card key={r.id} title={`Report ${r.id}: ${r.category}`}>
                    <StatusPill
                      status={
                        r.status_text === 'Work completed'
                          ? 'completed'
                          : 'in_progress'
                      }
                      label={r.status_text}
                    />
                    <p>
                      Reported <DateTime value={r.reported_on} format="date" />
                    </p>
                    <p>
                      Location: {r.location.lat}, {r.location.lng}
                    </p>
                    <a
                      className="link"
                      href={`https://www.openstreetmap.org/?mlat=${r.location.lat}&mlon=${r.location.lng}#map=16/${r.location.lat}/${r.location.lng}`}
                    >
                      View this location
                    </a>
                  </Card>
                ))}
              </section>
            ) : (
              <EmptyState
                title="No public road reports yet"
                description="Reported issues will appear here with their location and progress."
              />
            )}
          </div>
        )}
      </QueryView>
    </PageContainer>
  )
}
