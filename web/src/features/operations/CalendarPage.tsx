import { useState } from 'react'
import { Link } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { useMe, hasAnyRole } from '@/auth/useMe'
import {
  PageHeader,
  Card,
  Field,
  TextInput,
  Select,
  Button,
  DateTime,
  QueryView,
  EmptyState,
  ErrorAlert,
  Badge,
} from '@/ui'
import { addDays, localInput, today, utcInput } from './time'
import { useOperation } from './data'
import type { CalendarData, Resource } from './types'
export function CalendarPage() {
  const [from, setFrom] = useState(today())
  const [resource, setResource] = useState('')
  const me = useMe()
  const q = useQuery({
    queryKey: ['operations', 'calendar', from, resource],
    queryFn: () =>
      api.get<CalendarData>('/api/staff/calendar', {
        query: { from, to: addDays(from, 7), resource: resource || undefined },
      }),
  })
  return (
    <div className="space-y-5">
      <PageHeader
        title="Resource calendar"
        description="Norfolk Island time. Confirmed bookings occupy rooms including prep and cleanup; requests do not reserve space."
      />
      <div className="flex flex-wrap items-end gap-3">
        <Button variant="secondary" onClick={() => setFrom(addDays(from, -7))}>
          Previous week
        </Button>
        <Field label="Week starting">
          <TextInput
            type="date"
            value={from}
            onChange={(e) => e.target.value && setFrom(e.target.value)}
          />
        </Field>
        <Button variant="secondary" onClick={() => setFrom(addDays(from, 7))}>
          Next week
        </Button>
        <Field label="Resource">
          <Select
            value={resource}
            onChange={(e) => setResource(e.target.value)}
            placeholder="All resources"
            options={(q.data?.resources ?? []).map((r) => ({
              value: r.code,
              label: r.name,
            }))}
          />
        </Field>
      </div>
      <div className="flex flex-wrap gap-3">
        <Badge tone="primary">Confirmed</Badge>
        <Badge tone="warning">Requested</Badge>
        <Badge>Maintenance</Badge>
        <Badge tone="info">Shaded buffers</Badge>
      </div>
      <QueryView query={q}>
        {(data) => (
          <>
            <div className="overflow-x-auto rounded-xl border border-line">
              <table className="w-full min-w-[950px] border-collapse text-sm">
                <caption className="sr-only">
                  Weekly resource calendar, including buffers
                </caption>
                <thead>
                  <tr>
                    <th className="p-3 text-left">Resource</th>
                    {Array.from({ length: 7 }, (_, n) => (
                      <th key={n} className="p-3 text-left">
                        <DateTime value={addDays(from, n)} format="date" />
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {data.resources.map((r) => (
                    <tr key={r.id}>
                      <th
                        scope="row"
                        className="border-t border-line p-3 text-left"
                      >
                        {r.name}
                      </th>
                      {Array.from({ length: 7 }, (_, n) => {
                        const date = addDays(from, n)
                        const entries = data.entries.filter(
                          (e) =>
                            e.resource_id === r.id &&
                            localInput(e.start_at).slice(0, 10) <= date &&
                            localInput(e.end_at).slice(0, 10) >= date,
                        )
                        const requests = data.requests.filter(
                          (b) =>
                            localInput(b.start_at).slice(0, 10) === date &&
                            ((b.unit_code === 'rawson-whole' &&
                              r.kind === 'space') ||
                              (b.unit_code === 'rawson-main' &&
                                r.code === 'RAWSON_MAIN') ||
                              (b.unit_code === 'rawson-supper' &&
                                r.code === 'RAWSON_SUPPER')),
                        )
                        return (
                          <td
                            key={n}
                            className="min-w-36 border-l border-t border-line p-2 align-top"
                          >
                            {entries.map((e) => (
                              <div
                                key={e.id}
                                className={`mb-2 rounded border border-line ${e.source === 'maintenance' ? 'bg-sunken' : 'bg-primary-50'}`}
                              >
                                {e.event_start && (
                                  <p className="bg-sunken px-2 py-1 text-xs text-muted">
                                    Prep{' '}
                                    <DateTime
                                      value={e.start_at}
                                      format="time"
                                    />
                                    –
                                    <DateTime
                                      value={e.event_start}
                                      format="time"
                                    />
                                  </p>
                                )}
                                <div className="p-2">
                                  {e.case_id ? (
                                    <Link
                                      className="link"
                                      to={`/staff/cases/${e.case_id}`}
                                    >
                                      {e.label}
                                    </Link>
                                  ) : (
                                    e.label
                                  )}
                                  <p>
                                    <DateTime
                                      value={e.event_start ?? e.start_at}
                                      format="time"
                                    />
                                    –
                                    <DateTime
                                      value={e.event_end ?? e.end_at}
                                      format="time"
                                    />
                                  </p>
                                  {e.booking_id &&
                                    data.entries.some(
                                      (other) =>
                                        other.booking_id === e.booking_id &&
                                        other.resource_id !== e.resource_id,
                                    ) && (
                                      <p className="text-xs">
                                        Whole venue — both rooms
                                      </p>
                                    )}
                                </div>
                                {e.event_end && (
                                  <p className="bg-sunken px-2 py-1 text-xs text-muted">
                                    Cleanup{' '}
                                    <DateTime
                                      value={e.event_end}
                                      format="time"
                                    />
                                    –<DateTime value={e.end_at} format="time" />
                                  </p>
                                )}
                              </div>
                            ))}
                            {requests.map((b) => (
                              <div
                                key={b.id}
                                className="mb-2 rounded border border-dashed border-warning bg-warning-50 p-2"
                              >
                                <Badge tone="warning">Request</Badge>
                                <p>
                                  <Link
                                    className="link"
                                    to={`/staff/cases/${b.case_id}`}
                                  >
                                    {b.title}
                                  </Link>
                                </p>
                                <DateTime value={b.start_at} format="time" />–
                                <DateTime value={b.end_at} format="time" />
                              </div>
                            ))}
                            {!entries.length && !requests.length && (
                              <span className="text-muted">Free</span>
                            )}
                          </td>
                        )
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {!data.entries.length && !data.requests.length && (
              <EmptyState
                title="Nothing scheduled this week"
                description="All displayed resources are free."
              />
            )}
            {hasAnyRole(me.data, ['manager', 'sysadmin']) && (
              <MaintenanceForm resources={data.resources} />
            )}
          </>
        )}
      </QueryView>
    </div>
  )
}
export function MaintenanceForm({ resources }: { resources: Resource[] }) {
  const [resource, setResource] = useState(resources[0]?.code ?? '')
  const [start, setStart] = useState(`${today()}T07:00`)
  const [end, setEnd] = useState(`${today()}T12:00`)
  const [label, setLabel] = useState('')
  const op = useOperation()
  const errors = isApiError(op.error) ? op.error.fields : {}
  return (
    <Card title="Create maintenance block">
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Resource" required error={errors.resource_code}>
          <Select
            value={resource}
            onChange={(e) => setResource(e.target.value)}
            options={resources.map((r) => ({ value: r.code, label: r.name }))}
          />
        </Field>
        <Field label="Maintenance description" required error={errors.label}>
          <TextInput value={label} onChange={(e) => setLabel(e.target.value)} />
        </Field>
        <Field label="Start (Norfolk time)" required error={errors.start}>
          <TextInput
            type="datetime-local"
            value={start}
            onChange={(e) => setStart(e.target.value)}
          />
        </Field>
        <Field label="End (Norfolk time)" required error={errors.end}>
          <TextInput
            type="datetime-local"
            value={end}
            onChange={(e) => setEnd(e.target.value)}
          />
        </Field>
      </div>
      <Button
        className="mt-4"
        loading={op.isPending}
        onClick={() =>
          op.mutate({
            path: '/api/admin/maintenance',
            body: {
              resource_code: resource,
              start: utcInput(start),
              end: utcInput(end),
              label,
            },
          })
        }
      >
        Block time for maintenance
      </Button>
      {op.error && <ErrorAlert error={op.error} />}
    </Card>
  )
}
