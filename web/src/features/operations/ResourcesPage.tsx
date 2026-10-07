import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import {
  PageHeader,
  Card,
  Field,
  TextInput,
  Checkbox,
  Button,
  QueryView,
  ErrorAlert,
  useToast,
} from '@/ui'
import { MaintenanceForm } from './CalendarPage'
import type { Resource } from './types'
export function ResourcesPage() {
  const q = useQuery({
    queryKey: ['operations', 'admin-resources'],
    queryFn: () => api.get<Resource[]>('/api/admin/resources'),
  })
  return (
    <div className="space-y-5">
      <PageHeader
        title="Resources and maintenance"
        description="Buffers apply to new allocations. Existing confirmed bookings keep their allocated buffers."
      />
      <QueryView query={q}>
        {(rows) => (
          <>
            <div className="grid gap-4 md:grid-cols-2">
              {rows.map((r) => (
                <ResourceEditor key={r.id} resource={r} />
              ))}
            </div>
            <MaintenanceForm resources={rows} />
          </>
        )}
      </QueryView>
    </div>
  )
}
function ResourceEditor({ resource: r }: { resource: Resource }) {
  const [name, setName] = useState(r.name)
  const [prep, setPrep] = useState(r.prep_minutes)
  const [cleanup, setCleanup] = useState(r.cleanup_minutes)
  const [active, setActive] = useState(r.active === 1)
  const qc = useQueryClient()
  const toast = useToast()
  const m = useMutation({
    mutationFn: () =>
      api.patch(`/api/admin/resources/${r.id}`, {
        name,
        prep_minutes: prep,
        cleanup_minutes: cleanup,
        active,
      }),
    onSuccess: async () => {
      toast.success('Resource saved')
      await qc.invalidateQueries({ queryKey: ['operations'] })
    },
  })
  const errors = isApiError(m.error) ? m.error.fields : {}
  return (
    <Card title={r.code}>
      <div className="space-y-4">
        <Field label="Name" required error={errors.name}>
          <TextInput value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field
          label="Preparation buffer (minutes)"
          required
          error={errors.prep_minutes}
        >
          <TextInput
            type="number"
            min={0}
            max={1440}
            value={prep}
            onChange={(e) => setPrep(Number(e.target.value))}
          />
        </Field>
        <Field
          label="Cleanup buffer (minutes)"
          required
          error={errors.cleanup_minutes}
        >
          <TextInput
            type="number"
            min={0}
            max={1440}
            value={cleanup}
            onChange={(e) => setCleanup(Number(e.target.value))}
          />
        </Field>
        <Checkbox
          label="Active"
          checked={active}
          onChange={(e) => setActive(e.target.checked)}
        />
        <Button loading={m.isPending} onClick={() => m.mutate()}>
          Save resource
        </Button>
        {m.error && <ErrorAlert error={m.error} />}
      </div>
    </Card>
  )
}
