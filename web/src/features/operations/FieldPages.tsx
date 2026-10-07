import { useState } from 'react'
import { Link, useParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  PageHeader,
  Card,
  Checkbox,
  Button,
  Field,
  TextInput,
  Textarea,
  FileInput,
  DateTime,
  StatusPill,
  QueryView,
  ErrorAlert,
  Badge,
  EmptyState,
  Dialog,
} from '@/ui'
import { useOutbox } from './useOutbox'
import {
  fetchTask,
  fetchTasks,
  enqueue,
  discardTask,
  type Command,
} from './outbox'
import type { Task } from './types'
import { localInput, today, utcInput } from './time'

function OutboxStatus({
  commands,
  user,
}: {
  commands: Command[]
  user: number
}) {
  const [discard, setDiscard] = useState<number | null>(null)
  const [error, setError] = useState<unknown>(null)
  return (
    <div aria-live="polite" className="space-y-3">
      {commands.map((c) => (
        <div key={c.id} className="rounded-lg border border-line p-3">
          <Badge
            tone={
              c.state === 'confirmed'
                ? 'success'
                : c.state === 'conflict' || c.state === 'failed'
                  ? 'danger'
                  : 'warning'
            }
          >
            {
              {
                queued: 'Saved on this device',
                sending: 'Sending…',
                confirmed: 'Confirmed by server ✓',
                conflict: 'Conflict — review server version',
                failed: 'Could not apply — review update',
              }[c.state]
            }
          </Badge>
          <p>
            {String(c.body.kind ?? 'Equipment usage')} — saved{' '}
            <DateTime value={c.created} />
          </p>
          {c.error && <p>{c.error}</p>}
          {c.server && (
            <p>
              Server revision {c.server.revision}:{' '}
              <StatusPill status={c.server.status} /> {c.server.result_text}
            </p>
          )}
          {['conflict', 'failed'].includes(c.state) && (
            <Button variant="secondary" onClick={() => setDiscard(c.task)}>
              Keep server version and discard queued changes
            </Button>
          )}
        </div>
      ))}
      <Dialog
        open={discard !== null}
        onClose={() => setDiscard(null)}
        title="Discard local changes for this task?"
        footer={
          <Button
            variant="danger"
            onClick={() => {
              if (discard !== null)
                void discardTask(user, discard)
                  .then(() => setDiscard(null))
                  .catch(setError)
            }}
          >
            Discard queued changes
          </Button>
        }
      >
        <p>
          All unapplied changes to this task saved on this device will be
          removed. The server record will be kept. You can review it and enter a
          new update.
        </p>
        {error != null && <ErrorAlert error={error} />}
      </Dialog>
    </div>
  )
}
export function FieldPage() {
  const box = useOutbox()
  const q = useQuery({
    queryKey: ['operations', 'field', box.user],
    queryFn: () => fetchTasks(box.user),
    enabled: !!box.user,
    networkMode: 'always',
  })
  return (
    <div className="space-y-5">
      <PageHeader
        title="Field tasks"
        description="Your assigned work. Open tasks while online to save them on this device."
      />
      <Checkbox
        label="Simulate offline (demo)"
        checked={box.offline}
        onChange={(e) => box.setOffline(e.target.checked)}
      />
      <Button variant="secondary" onClick={box.send} disabled={box.offline}>
        Sync saved updates
      </Button>
      {box.error != null && <ErrorAlert error={box.error} />}
      <QueryView query={q}>
        {(rows) =>
          rows.length ? (
            <div className="space-y-3">
              {['Today', 'Upcoming', 'Earlier or unscheduled'].map((group) => {
                const tasks = rows.filter((t) => {
                  const date = t.scheduled_start
                    ? localInput(t.scheduled_start).slice(0, 10)
                    : ''
                  return group === 'Today'
                    ? date === today()
                    : group === 'Upcoming'
                      ? date > today()
                      : date < today()
                })
                return tasks.length ? (
                  <section key={group}>
                    <h2 className="mb-3 text-xl font-semibold">{group}</h2>
                    {tasks.map((t) => (
                      <Card
                        key={t.id}
                        title={
                          <Link
                            className="link block min-h-11"
                            to={`/staff/field/${t.id}`}
                          >
                            {t.title}
                          </Link>
                        }
                      >
                        <StatusPill status={t.status} />
                        <p>
                          <DateTime value={t.scheduled_start} />
                        </p>
                        <p>{t.location_text}</p>
                      </Card>
                    ))}
                  </section>
                ) : null
              })}
            </div>
          ) : (
            <EmptyState
              title="No tasks assigned to you"
              description="Assigned tasks will appear here with the location and instructions you need."
            />
          )
        }
      </QueryView>
      <OutboxStatus commands={box.commands} user={box.user} />
    </div>
  )
}
export function FieldTaskPage() {
  const id = Number(useParams().id)
  const box = useOutbox()
  const q = useQuery({
    queryKey: ['operations', 'field-task', box.user, id],
    queryFn: () => fetchTask(box.user, id),
    enabled: !!box.user,
    networkMode: 'always',
  })
  return (
    <div className="space-y-5">
      <PageHeader
        title="Field task"
        breadcrumbs={[
          { label: 'Field tasks', to: '/staff/field' },
          { label: 'Task detail' },
        ]}
      />
      <Checkbox
        label="Simulate offline (demo)"
        checked={box.offline}
        onChange={(e) => box.setOffline(e.target.checked)}
      />
      <QueryView query={q}>
        {(t) => (
          <TaskContent
            task={t}
            user={box.user}
            sync={() => {
              if (!box.offline) box.send()
            }}
          />
        )}
      </QueryView>
      <OutboxStatus
        commands={box.commands.filter((c) => c.task === id)}
        user={box.user}
      />
      {box.error != null && <ErrorAlert error={box.error} />}
    </div>
  )
}
function TaskContent({
  task: t,
  user,
  sync,
}: {
  task: Task
  user: number
  sync: () => void
}) {
  const qc = useQueryClient()
  const [body, setBody] = useState('')
  const [result, setResult] = useState(t.result_text ?? '')
  const [photo, setPhoto] = useState<File | null>(null)
  const [error, setError] = useState<unknown>(null)
  const [busy, setBusy] = useState(false)
  const closed = ['done', 'cancelled'].includes(t.status)
  const save = async (value: Record<string, unknown>, usage = false) => {
    setBusy(true)
    setError(null)
    try {
      await enqueue(user, t, value, usage)
      await qc.invalidateQueries({ queryKey: ['operations'] })
      sync()
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }
  const savePhoto = async () => {
    if (!photo) return
    if (photo.size > 8 * 1024 * 1024) {
      setError(new Error('Choose a photo under 8 MB.'))
      return
    }
    const data = await new Promise<string>((resolve, reject) => {
      const r = new FileReader()
      r.onload = () => resolve(String(r.result).split(',')[1] ?? '')
      r.onerror = () => reject(r.error)
      r.readAsDataURL(photo)
    })
    await save({
      kind: 'photo',
      body,
      photo: { name: photo.name, base64: data },
    })
  }
  return (
    <div className="space-y-5">
      <Card title={t.title}>
        <StatusPill status={t.status} />
        <p>
          <DateTime value={t.scheduled_start} />–
          <DateTime value={t.scheduled_end} format="time" />
        </p>
        <p>{t.location_text}</p>
        {t.location_lat !== null && t.location_lng !== null && (
          <a
            className="link"
            href={`https://www.openstreetmap.org/?mlat=${t.location_lat}&mlon=${t.location_lng}#map=16/${t.location_lat}/${t.location_lng}`}
            target="_blank"
            rel="noreferrer"
          >
            Open location map
          </a>
        )}
        <p className="mt-3 whitespace-pre-line">{t.instructions}</p>
      </Card>
      <Card title="Checklist">
        <div className="space-y-4">
          {t.checklist.map((c) => (
            <Checkbox
              key={c.key}
              label={c.label}
              checked={c.done}
              disabled={closed || busy}
              onChange={(e) =>
                void save({
                  kind: 'checklist',
                  body: JSON.stringify({ key: c.key, done: e.target.checked }),
                })
              }
            />
          ))}
        </div>
      </Card>
      {!closed && (
        <>
          <Card title="Note or photo">
            <div className="space-y-4">
              <Field label="Note">
                <Textarea
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                />
              </Field>
              <Button
                loading={busy}
                disabled={!body.trim()}
                onClick={() => void save({ kind: 'note', body })}
              >
                Save note on this device
              </Button>
              <Field label="Photo (up to 8 MB)">
                <FileInput
                  accept="image/jpeg,image/png,image/webp"
                  onChange={(e) => setPhoto(e.target.files?.[0] ?? null)}
                />
              </Field>
              <Button
                disabled={!photo || busy}
                onClick={() => void savePhoto().catch(setError)}
              >
                Save photo on this device
              </Button>
            </div>
          </Card>
          {t.kind === 'equipment_job' && (
            <UsageForm save={(value) => save(value, true)} busy={busy} />
          )}
          <Card title="Work result">
            <Field label="Result" required>
              <Textarea
                value={result}
                onChange={(e) => setResult(e.target.value)}
              />
            </Field>
            <div className="mt-4 flex flex-col gap-3">
              <Button
                loading={busy}
                disabled={!result.trim()}
                onClick={() => void save({ kind: 'result', body: result })}
              >
                Save result
              </Button>
              <Button
                variant="secondary"
                disabled={busy}
                onClick={() =>
                  void save({ kind: 'status', body: 'in_progress' })
                }
              >
                Mark in progress
              </Button>
              <Button
                disabled={
                  busy || !t.result_text || t.checklist.some((c) => !c.done)
                }
                onClick={() => void save({ kind: 'status', body: 'done' })}
              >
                Mark done
              </Button>
            </div>
            <p className="mt-2 text-sm text-muted">
              Complete the checklist and save a result first. Equipment work
              also requires a job card.
            </p>
          </Card>
        </>
      )}
      {error != null && <ErrorAlert error={error} />}
      <Card title="Previous updates">
        {t.updates.length ? (
          t.updates.map((u) => (
            <div key={u.id} className="border-b border-line py-3">
              <strong>{u.kind}</strong> —{' '}
              <DateTime value={u.created_offline_at ?? u.created_at} />
              <p className="whitespace-pre-line">
                {u.kind === 'checklist' ? 'Checklist updated' : u.body}
              </p>
              {u.blob_id && (
                <a
                  className="link"
                  href={`/api/field/tasks/${t.id}/photos/${u.id}`}
                >
                  View photo
                </a>
              )}
            </div>
          ))
        ) : (
          <EmptyState title="No previous updates" />
        )}
      </Card>
    </div>
  )
}
function UsageForm({
  save,
  busy,
}: {
  save: (value: Record<string, unknown>) => Promise<void>
  busy: boolean
}) {
  const [start, setStart] = useState(`${today()}T07:30`)
  const [end, setEnd] = useState(`${today()}T13:30`)
  const [down, setDown] = useState(30)
  const [expenses, setExpenses] = useState(0)
  const [note, setNote] = useState('')
  return (
    <Card title="Equipment job card">
      <div className="space-y-4">
        <Field label="Depot departure (Norfolk time)" required>
          <TextInput
            type="datetime-local"
            value={start}
            onChange={(e) => setStart(e.target.value)}
          />
        </Field>
        <Field label="Depot return (Norfolk time)" required>
          <TextInput
            type="datetime-local"
            value={end}
            onChange={(e) => setEnd(e.target.value)}
          />
        </Field>
        <Field label="Downtime minutes" required>
          <TextInput
            type="number"
            min={0}
            value={down}
            onChange={(e) => setDown(Number(e.target.value))}
          />
        </Field>
        <Field label="Agreed expenses (AUD cents)">
          <TextInput
            type="number"
            min={0}
            value={expenses}
            onChange={(e) => setExpenses(Number(e.target.value))}
          />
        </Field>
        <Field label="Explain agreed expenses">
          <Textarea value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        <Button
          loading={busy}
          onClick={() =>
            void save({
              started_at: utcInput(start),
              ended_at: utcInput(end),
              downtime_minutes: down,
              expenses_cents: expenses,
              expenses_note: note,
            })
          }
        >
          Save job card on this device
        </Button>
      </div>
    </Card>
  )
}
