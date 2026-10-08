import { useState } from 'react'
import { Link } from 'react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import {
  Button,
  Card,
  QueryView,
  DateTime,
  StatusPill,
  Field,
  TextInput,
  Textarea,
  Select,
  Dialog,
  ErrorAlert,
  Money,
  EmptyState,
  Table,
  formatDateTime,
  useToast,
} from '@/ui'
import { BookingSlot } from './fields'
import { useOperation } from './data'
import { utcInput, priceNote } from './time'
import type {
  Availability,
  BookingDetail,
  Equipment,
  Preview,
  Slot,
  Task,
  Resources,
} from './types'

export function BookingPanel({ caseId }: { caseId: number }) {
  const q = useQuery({
    queryKey: ['operations', 'booking', caseId],
    queryFn: () => api.get<BookingDetail>(`/api/cases/${caseId}/booking`),
  })
  return (
    <QueryView query={q}>
      {(d) => <BookingContent caseId={caseId} data={d} />}
    </QueryView>
  )
}
function BookingContent({
  caseId,
  data: d,
}: {
  caseId: number
  data: BookingDetail
}) {
  const op = useOperation()
  const qc = useQueryClient()
  const toast = useToast()
  // Staff always receive `can_manage`; the applicant projection omits it.
  const staff = d.can_manage !== undefined
  const manager = d.can_manage === true
  const [dialog, setDialog] = useState<'move' | 'cancel' | null>(null)
  const [slot, setSlot] = useState<Slot>({
    unit_code: d.unit.code,
    start_at: d.booking.start_at,
    end_at: d.booking.end_at,
    attendees: d.booking.attendees,
  })
  const [reason, setReason] = useState('')
  const [note, setNote] = useState('')
  const preview = useMutation({
    mutationFn: () =>
      api.post<Preview>(
        `/api/cases/${caseId}/booking/reschedule/preview`,
        moveBody,
      ),
  })
  const moveBody = {
    unit_code: slot.unit_code,
    start: slot.start_at,
    end: slot.end_at,
    reason: manager ? reason : '',
    expected_revision: d.booking.revision,
  }
  // The applicant cannot move a booking; the request goes to Council through the case conversation.
  const ask = useMutation({
    mutationFn: () => {
      const unit =
        slot.unit_code === d.unit.code
          ? d.unit.name
          : (qc
              .getQueriesData<Availability>({ queryKey: ['operations', 'availability'] })
              .flatMap(([, a]) => a?.units ?? [])
              .find((u) => u.code === slot.unit_code)?.name ?? slot.unit_code)
      return api.post(`/api/cases/${caseId}/messages`, {
        body: [
          `Please move my booking to ${unit}, ${formatDateTime(slot.start_at)}–${formatDateTime(slot.end_at, 'time')}.`,
          note.trim(),
        ]
          .filter(Boolean)
          .join('\n\n'),
      })
    },
    onSuccess: async () => {
      toast.success('Your request to move the booking was sent to Council.')
      setDialog(null)
      setNote('')
      await qc.invalidateQueries()
    },
  })
  const errors = isApiError(op.error) ? op.error.fields : {}
  const active = ['requested', 'confirmed'].includes(d.booking.status)
  const moveTitle = manager
    ? 'Reschedule booking'
    : staff
      ? 'Preview a reschedule'
      : 'Preview a new time'
  return (
    <div className="space-y-5">
      <Card title={d.unit.name}>
        <p>
          <DateTime value={d.booking.start_at} />–
          <DateTime value={d.booking.end_at} format="time" />
        </p>
        <StatusPill status={d.booking.status} />
        <p>{d.booking.attendees} guests</p>
        {d.booking.status === 'requested' && (
          <p>
            {staff
              ? 'Requested by the applicant — awaiting confirmation.'
              : 'Your booking is requested and awaits Council confirmation.'}
          </p>
        )}
        {d.booking.confirmation_version_id && (
          <a
            className="link"
            href={`/api/cases/${caseId}/booking/confirmation/${d.booking.confirmation_version_id}`}
          >
            Download booking confirmation PDF
          </a>
        )}
        {d.conflicts && (
          <div aria-live="polite" className="mt-4">
            <strong>
              Availability: {d.conflicts.length ? 'Conflicts found' : 'Free'}
            </strong>
            {d.conflicts.map((c, i) => (
              <p key={i}>
                {c.label}: <DateTime value={c.start_at} />–
                <DateTime value={c.end_at} />
              </p>
            ))}
          </div>
        )}
        {d.can_manage && active && (
          <div className="mt-4 flex flex-wrap gap-3">
            {d.booking.status === 'requested' && (
              <Button
                loading={op.isPending}
                disabled={!d.settled || !!d.conflicts?.length}
                onClick={() =>
                  op.mutate({
                    path: `/api/cases/${caseId}/booking/confirm`,
                    body: { expected_revision: d.booking.revision },
                  })
                }
              >
                Confirm booking
              </Button>
            )}
            <Button
              variant="secondary"
              onClick={() => {
                preview.reset()
                setDialog('move')
              }}
            >
              Reschedule
            </Button>
            <Button variant="danger-outline" onClick={() => setDialog('cancel')}>
              Cancel booking (unused)
            </Button>
          </div>
        )}
        {!manager && active && (
          <Button
            variant="secondary"
            onClick={() => {
              preview.reset()
              ask.reset()
              setDialog('move')
            }}
          >
            {staff ? 'Preview a reschedule' : 'Preview a new time'}
          </Button>
        )}
        {d.can_manage && d.booking.status === 'requested' && !d.settled && (
          <p className="mt-2">
            Confirmation is available when hire fees and bond are received.
          </p>
        )}
      </Card>
      {op.error && <ErrorAlert error={op.error} />}
      <Card title="Conditions of hire">
        <p className="text-sm">{d.conditions}</p>
      </Card>
      <Table
        caption="Booking revision history"
        rows={d.history}
        rowKey={(r) => r.revision}
        columns={[
          { key: 'rev', header: 'Revision', cell: (r) => r.revision },
          {
            key: 'space',
            header: 'Space and time',
            cell: (r) => (
              <>
                {r.unit}
                <br />
                <DateTime value={r.start_at} />–
                <DateTime value={r.end_at} format="time" />
              </>
            ),
          },
          {
            key: 'status',
            header: 'Status',
            cell: (r) => <StatusPill status={r.status} />,
          },
          { key: 'reason', header: 'What changed', cell: (r) => r.reason },
        ]}
      />
      <Dialog
        open={dialog !== null}
        onClose={() => setDialog(null)}
        title={dialog === 'move' ? moveTitle : 'Cancel booking'}
        footer={
          manager ? (
            <Button
              loading={op.isPending}
              variant={dialog === 'move' ? 'primary' : 'danger'}
              disabled={dialog === 'move' ? !preview.data?.available : !reason.trim()}
              onClick={() =>
                op.mutate(
                  {
                    path: `/api/cases/${caseId}/booking/${dialog === 'move' ? 'reschedule' : 'cancel'}`,
                    body:
                      dialog === 'move'
                        ? moveBody
                        : { reason, expected_revision: d.booking.revision },
                  },
                  { onSuccess: () => setDialog(null) },
                )
              }
            >
              {dialog === 'move' ? 'Save reschedule' : 'Cancel booking (unused)'}
            </Button>
          ) : (
            <>
              <Button variant="secondary" onClick={() => setDialog(null)}>
                Close
              </Button>
              {!staff && (
                <Button
                  loading={ask.isPending}
                  disabled={!preview.data?.available}
                  onClick={() => ask.mutate()}
                >
                  Ask Council to move my booking
                </Button>
              )}
            </>
          )
        }
      >
        <div className="space-y-4">
          {dialog === 'move' && (
            <>
              <BookingSlot
                field={{
                  key: 'slot',
                  type: 'booking_slot',
                  label: 'New booking slot',
                  required: true,
                }}
                value={slot}
                onChange={(v) => {
                  setSlot(v as Slot)
                  preview.reset()
                }}
                error={
                  errors.start_at ??
                  errors.end_at ??
                  errors.unit_code ??
                  errors.attendees
                }
              />
              <Button
                variant="secondary"
                loading={preview.isPending}
                onClick={() => preview.mutate()}
              >
                Preview availability and fees
              </Button>
              {preview.error && <ErrorAlert error={preview.error} />}
              {preview.data && (
                <div aria-live="polite">
                  <p>
                    {preview.data.available
                      ? 'Free for the new times.'
                      : 'Unavailable; choose another slot.'}
                  </p>
                  {preview.data.conflicts.map((c, i) => (
                    <p key={i}>{c.label}</p>
                  ))}
                  <p>{priceNote}</p>
                  {preview.data.old_lines.map((l,i)=><p key={`old-${i}`}>Previous: {l.description} <Money cents={l.amount_cents}/></p>)}
                  {preview.data.new_lines.map((l,i)=><p key={`new-${i}`}>Proposed: {l.description} <Money cents={l.amount_cents}/></p>)}
                  <p>Credit change: <Money cents={preview.data.credit_delta_cents}/></p>
                  {staff && !manager && (
                    <p>Only Customer Care can reschedule this booking.</p>
                  )}
                  {!staff && (
                    <p>
                      Your booking does not change until Council checks and
                      confirms the new time.
                    </p>
                  )}
                  <p>
                    Previous fees and bond:{' '}
                    <Money
                      cents={preview.data.old_lines.reduce(
                        (n, l) => n + l.amount_cents,
                        0,
                      )}
                    />
                  </p>
                  <p>
                    New fees and bond:{' '}
                    <Money
                      cents={preview.data.new_lines.reduce(
                        (n, l) => n + l.amount_cents,
                        0,
                      )}
                    />
                  </p>
                  <p>
                    Finance will issue any price adjustment. Previous revisions
                    remain in history.
                  </p>
                </div>
              )}
            </>
          )}
          {manager && (
            <Field label="Reason" required error={errors.reason}>
              <Textarea
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </Field>
          )}
          {!staff && dialog === 'move' && (
            <Field
              label="Message to Council (optional)"
              hint="Sent with your request in the request messages."
            >
              <Textarea value={note} onChange={(e) => setNote(e.target.value)} />
            </Field>
          )}
          {op.error && <ErrorAlert error={op.error} />}
          {ask.error && <ErrorAlert error={ask.error} />}
        </div>
      </Dialog>
    </div>
  )
}
export function TasksPanel({ caseId }: { caseId: number }) {
  const q = useQuery({
    queryKey: ['operations', 'tasks', caseId],
    queryFn: () => api.get<Task[]>(`/api/cases/${caseId}/tasks`),
  })
  const resources = useQuery({
    queryKey: ['operations', 'resources'],
    queryFn: () => api.get<Resources>('/api/staff/operations/resources'),
  })
  return (
    <QueryView query={q}>
      {(rows) =>
        rows.length ? (
          <div className="space-y-4">
            {rows.map((t) => (
              <StaffTask
                key={t.id}
                task={t}
                workers={resources.data?.workers ?? []}
              />
            ))}
          </div>
        ) : (
          <EmptyState
            title="No field tasks yet"
            description="Tasks appear when the request reaches a field work step."
          />
        )
      }
    </QueryView>
  )
}
function StaffTask({
  task: t,
  workers,
}: {
  task: Task
  workers: Resources['workers']
}) {
  const [worker, setWorker] = useState(String(t.assigned_to ?? ''))
  const [reason, setReason] = useState('')
  const [cancelOpen, setCancelOpen] = useState(false)
  const op = useOperation()
  return (
    <Card title={t.title}>
      <StatusPill status={t.status} />
      <p className="mt-2">
        Assigned to:{' '}
        {t.assigned_to === null
          ? 'Not assigned yet'
          : (workers.find((w) => w.id === t.assigned_to)?.name ?? `Staff member #${t.assigned_to}`)}
      </p>
      {t.scheduled_start && (
        <p>
          Scheduled start: <DateTime value={t.scheduled_start} />
        </p>
      )}
      <p className="whitespace-pre-line">{t.instructions}</p>
      <Link className="link" to={`/staff/field/${t.id}`}>
        Open task
      </Link>
      {t.can_manage && !['done', 'cancelled'].includes(t.status) && (
        <div className="mt-4 space-y-3">
          <Field label="Assign field worker">
            <Select
              value={worker}
              onChange={(e) => setWorker(e.target.value)}
              placeholder="Choose a worker"
              options={workers.map((w) => ({
                value: String(w.id),
                label: w.name,
              }))}
            />
          </Field>
          <Button
            variant="secondary"
            loading={op.isPending}
            disabled={!worker}
            onClick={() =>
              op.mutate({
                path: `/api/tasks/${t.id}/assign`,
                body: {
                  assigned_to: Number(worker),
                  expected_revision: t.revision,
                },
              })
            }
          >
            Assign
          </Button>
          <Button variant="danger-outline" onClick={() => { setReason(''); setCancelOpen(true) }}>Cancel task</Button>
          <Dialog open={cancelOpen} onClose={() => { if (!op.isPending) setCancelOpen(false) }} title="Cancel task?" description="Record why this task is no longer needed." footer={<>
            <Button variant="secondary" disabled={op.isPending} onClick={() => setCancelOpen(false)}>Back</Button>
            <Button variant="danger" loading={op.isPending} disabled={!reason.trim()} onClick={() => op.mutate({ path: `/api/tasks/${t.id}/cancel`, body: { reason, expected_revision: t.revision } }, { onSuccess: () => setCancelOpen(false) })}>Cancel task</Button>
          </>}>
            <Field label="Cancellation reason" required><Textarea value={reason} onChange={(e) => setReason(e.target.value)} /></Field>
            {op.error && <ErrorAlert error={op.error} />}
          </Dialog>
        </div>
      )}
      {op.error && <ErrorAlert error={op.error} />}
    </Card>
  )
}
export function EquipmentPanel({ caseId }: { caseId: number }) {
  const q = useQuery({
    queryKey: ['operations', 'equipment', caseId],
    queryFn: () => api.get<Equipment>(`/api/cases/${caseId}/equipment`),
  })
  return (
    <QueryView query={q}>
      {(d) => <EquipmentContent data={d} caseId={caseId} />}
    </QueryView>
  )
}
function EquipmentContent({
  data: d,
  caseId,
}: {
  data: Equipment
  caseId: number
}) {
  const op = useOperation()
  const r = useQuery({
    queryKey: ['operations', 'resources'],
    queryFn: () => api.get<Resources>('/api/staff/operations/resources'),
    enabled: d.can_schedule,
  })
  const [plant, setPlant] = useState('')
  const [worker, setWorker] = useState('')
  const [start, setStart] = useState(`${d.request.preferred_date}T07:30`)
  const [end, setEnd] = useState(`${d.request.preferred_date}T11:30`)
  const errors = isApiError(op.error) ? op.error.fields : {}
  return (
    <div className="space-y-4">
      <Card title="Requested equipment">
        <p>{d.request.description}</p>
        <p>{d.request.site_text}</p>
        <p>
          Requested {d.request.requested_hours} hours on{' '}
          <DateTime value={d.request.preferred_date} format="date" />.
        </p>
        <p>
          Scheduled: <DateTime value={d.request.scheduled_start} />
        </p>
      </Card>
      <Card title="Estimate and actual charge">
        <p className="text-sm text-muted">{priceNote}</p>
        <p>
          An estimate uses requested hours and provisional plant. The final
          invoice uses approved job-card minutes plus agreed expenses.
        </p>
        {d.invoices.length ? (
          d.invoices.map((i) => (
            <div key={i.id} className="mt-3 border-t border-line pt-3">
              <strong>
                {i.kind === 'estimate' ? 'Estimate' : 'Final invoice'}{' '}
                {i.number}
              </strong>
              : <Money cents={i.total_cents} />
              <p>{i.basis_note}</p>
              <Link className="link" to={`?tab=finance.money`}>
                View invoice and payment details
              </Link>
            </div>
          ))
        ) : (
          <EmptyState title="No estimate issued yet" />
        )}
      </Card>
      <Card title="Actual job cards">
        {d.usage.length ? (
          d.usage.map((u) => (
            <div key={u.id} className="space-y-2 border-b border-line py-3">
              <p>
                <DateTime value={u.started_at} />–
                <DateTime value={u.ended_at} format="time" />
              </p>
              <p>
                Billable: {Math.floor(u.billable_minutes / 60)} h{' '}
                {u.billable_minutes % 60} min; downtime: {u.downtime_minutes}{' '}
                min. Expenses: <Money cents={u.expenses_cents} />.
              </p>
              <StatusPill status={u.approved_at ? 'approved' : 'pending'} />
              {d.can_approve && !u.approved_at && (
                <Button
                  loading={op.isPending}
                  onClick={() =>
                    op.mutate({
                      path: `/api/cases/${caseId}/equipment/usage/${u.id}/approve`,
                    })
                  }
                >
                  Approve job card and issue invoice
                </Button>
              )}
            </div>
          ))
        ) : (
          <EmptyState
            title="No job card recorded yet"
            description="The operator records actual usage after the work."
          />
        )}
      </Card>
      {d.can_schedule && (
        <Card title="Schedule Council plant">
          <div className="space-y-4">
            <Field label="Plant" required error={errors.resource_code}>
              <Select
                placeholder="Choose plant"
                value={plant}
                onChange={(e) => setPlant(e.target.value)}
                options={(r.data?.resources ?? [])
                  .filter((x) => x.kind === 'equipment')
                  .map((x) => ({ value: x.code, label: x.name }))}
              />
            </Field>
            <Field
              label="Operator"
              required
              error={errors.operator_user_id ?? errors.assigned_to}
            >
              <Select
                placeholder="Choose an operator"
                value={worker}
                onChange={(e) => setWorker(e.target.value)}
                options={(r.data?.workers ?? []).map((x) => ({
                  value: String(x.id),
                  label: x.name,
                }))}
              />
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
            <Button
              loading={op.isPending}
              onClick={() =>
                op.mutate({
                  path: `/api/cases/${caseId}/equipment/schedule`,
                  body: {
                    resource_code: plant,
                    operator_user_id: Number(worker),
                    start: utcInput(start),
                    end: utcInput(end),
                    expected_revision: d.revision,
                  },
                })
              }
            >
              Schedule
            </Button>
            {r.error && <ErrorAlert error={r.error} />}
          </div>
        </Card>
      )}
      {op.error && <ErrorAlert error={op.error} />}
    </div>
  )
}

export function RoadResponsePanel({ caseId }: { caseId: number }) {
  const q = useQuery({
    queryKey: ['operations', 'road-response', caseId],
    queryFn: () =>
      api.get<{ revision: number; can_issue: boolean; issued: boolean; location: string }>(
        `/api/cases/${caseId}/road-response`,
      ),
  })
  const op = useOperation()
  const [body, setBody] = useState('')
  const errors = isApiError(op.error) ? op.error.fields : {}
  return (
    <QueryView query={q}>
      {(d) => (
        <Card title="Council road issue response">
          <p>{d.location}</p>
          {!d.can_issue && (
            <p className="mt-4 text-sm">
              {d.issued
                ? 'The response letter has been issued. The resident can download it from Documents.'
                : 'The response letter can be issued once the case reaches the response step.'}
            </p>
          )}
          {d.can_issue && (
            <div className="mt-4 space-y-4">
              <Field
                label="Response for the resident"
                required
                error={errors.body}
                hint="The response becomes an issued PDF letter in the case."
              >
                <Textarea
                  value={body}
                  maxLength={12000}
                  onChange={(e) => setBody(e.target.value)}
                />
              </Field>
              <Button
                loading={op.isPending}
                onClick={() =>
                  op.mutate({
                    path: `/api/cases/${caseId}/road-response`,
                    body: { body, expected_revision: d.revision },
                  })
                }
              >
                Issue response letter
              </Button>
            </div>
          )}
          {op.error && <ErrorAlert error={op.error} />}
        </Card>
      )}
    </QueryView>
  )
}
