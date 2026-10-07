import { api, isApiError, newIdempotencyKey } from '@/api/client'
import type { Me } from '@/api/types'
import type { Task } from './types'
export interface Command {
  id: string
  user: number
  task: number
  path: string
  body: Record<string, unknown>
  state: 'queued' | 'sending' | 'confirmed' | 'conflict' | 'failed'
  error?: string
  server?: Task
  created: string
}
const events = new EventTarget()
export function subscribe(listener: () => void) {
  events.addEventListener('change', listener)
  return () => events.removeEventListener('change', listener)
}
const changed = () => events.dispatchEvent(new Event('change'))
let opening: Promise<IDBDatabase> | undefined
function database() {
  opening ??= new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open('norfolk-operations', 1)
    request.onupgradeneeded = () => {
      request.result.createObjectStore('commands', { keyPath: 'id' })
      request.result.createObjectStore('tasks')
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () =>
      reject(
        new Error(
          'Device storage is unavailable. Enable browser storage before working offline.',
        ),
      )
  })
  return opening
}
async function read<T>(store: string, key?: string): Promise<T> {
  const db = await database()
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(store, 'readonly')
    const request = key
      ? transaction.objectStore(store).get(key)
      : transaction.objectStore(store).getAll()
    request.onsuccess = () => resolve(request.result as T)
    request.onerror = () => reject(request.error)
  })
}
async function write(store: string, value: unknown, key?: string) {
  const db = await database()
  await new Promise<void>((resolve, reject) => {
    const transaction = db.transaction(store, 'readwrite')
    if (key) transaction.objectStore(store).put(value, key)
    else transaction.objectStore(store).put(value)
    transaction.oncomplete = () => resolve()
    transaction.onerror = () => reject(transaction.error)
    transaction.onabort = () => reject(transaction.error)
  })
}
export async function commands(user: number) {
  return (await read<Command[]>('commands'))
    .filter((c) => c.user === user)
    .sort(
      (a, b) => a.created.localeCompare(b.created) || a.id.localeCompare(b.id),
    )
}
export async function cachedTasks(user: number): Promise<Task[]> {
  return (await read<{ user: number; task: Task }[]>('tasks'))
    .filter((x) => x.user === user)
    .map((x) => x.task)
}
export async function cacheTask(user: number, task: Task) {
  await write('tasks', { user, task }, `${user}:${task.id}`)
}
export async function fetchTasks(user: number) {
  const pending = await commands(user)
  try {
    const rows = await api.get<Task[]>('/api/field/tasks')
    for (const task of rows)
      if (!pending.some((c) => c.task === task.id && c.state !== 'confirmed'))
        await cacheTask(user, task)
    const allowed = new Set(rows.map((t) => t.id))
    const cached = await cachedTasks(user)
    const db = await database()
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction('tasks', 'readwrite')
      for (const t of cached)
        if (!allowed.has(t.id))
          tx.objectStore('tasks').delete(`${user}:${t.id}`)
      tx.oncomplete = () => resolve()
      tx.onerror = () => reject(tx.error)
    })
    return cached.filter((t) => allowed.has(t.id))
  } catch (e) {
    if (isApiError(e, 'network')) {
      const cached = await cachedTasks(user)
      if (cached.length) return cached
    }
    throw e
  }
}
export async function fetchTask(user: number, id: number) {
  const pending = await commands(user)
  const cached = (await cachedTasks(user)).find((t) => t.id === id)
  try {
    const task = await api.get<Task>(`/api/field/tasks/${id}`)
    if (cached && pending.some((c) => c.task === id && c.state !== 'confirmed'))
      return cached
    await cacheTask(user, task)
    return task
  } catch (e) {
    if (cached && isApiError(e, 'network')) return cached
    throw e
  }
}
export function optimistic(task: Task, body: Record<string, unknown>): Task {
  const next = structuredClone(task)
  if (body.kind === 'status') next.status = String(body.body)
  if (body.kind === 'result') next.result_text = String(body.body)
  if (body.kind === 'checklist') {
    const change = JSON.parse(String(body.body)) as {
      key: string
      done: boolean
    }
    next.checklist = next.checklist.map((c) =>
      c.key === change.key ? { ...c, done: change.done } : c,
    )
  }
  if (body.kind) next.revision++
  return next
}
export async function enqueue(
  user: number,
  task: Task,
  body: Record<string, unknown>,
  usage = false,
) {
  if (
    (await commands(user)).some(
      (c) => c.task === task.id && ['conflict', 'failed'].includes(c.state),
    )
  )
    throw new Error(
      'Review the queued conflict before adding more changes to this task.',
    )
  const id = newIdempotencyKey()
  // Timestamp order is made monotonic per browser to preserve revision-dependent commands.
  const previous = (await commands(user)).at(-1)
  const created = new Date(
    Math.max(Date.now(), previous ? Date.parse(previous.created) + 1 : 0),
  ).toISOString()
  const value: Command = {
    id,
    user,
    task: task.id,
    path: `/api/field/tasks/${task.id}/${usage ? 'usage' : 'updates'}`,
    body: {
      ...body,
      expected_revision: task.revision,
      client_command_id: id,
      created_offline_at: new Date().toISOString(),
    },
    state: 'queued',
    created,
  }
  // Persist command and optimistic projection in one transaction, so a reload cannot lose either.
  const db = await database()
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(['commands', 'tasks'], 'readwrite')
    tx.objectStore('commands').put(value)
    tx.objectStore('tasks').put(
      { user, task: optimistic(task, body) },
      `${user}:${task.id}`,
    )
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
    tx.onabort = () => reject(tx.error)
  })
  changed()
}
let syncing: Promise<void> | null = null
export function sync(user: number, simulateOffline: boolean): Promise<void> {
  if (simulateOffline || !navigator.onLine) return Promise.resolve()
  if (syncing) return syncing
  syncing = runSync(user).finally(() => {
    syncing = null
    changed()
  })
  return syncing
}
async function runSync(user: number) {
  const pending = await commands(user)
  const blocked = new Set(
    pending
      .filter((c) => ['conflict', 'failed'].includes(c.state))
      .map((c) => c.task),
  )
  for (const c of pending) {
    if (!['queued', 'sending'].includes(c.state) || blocked.has(c.task))
      continue
    const me = await api.get<Me>('/api/me')
    if (me.user?.id !== user || me.mfa_required) return
    c.state = 'sending'
    await write('commands', c)
    changed()
    try {
      const result = await api.post<{ task?: Task }>(c.path, c.body, {
        idempotencyKey: c.id,
      })
      c.state = 'confirmed'
      if (result.task) {
        // Remaining queued commands retain the optimistic projection until they are all confirmed.
        const remaining = (await commands(user)).some(
          (other) =>
            other.task === c.task &&
            other.id !== c.id &&
            other.state !== 'confirmed',
        )
        if (!remaining) await cacheTask(user, result.task)
      }
      delete c.body.photo
    } catch (e) {
      if (isApiError(e, 'network')) {
        c.state = 'queued'
        await write('commands', c)
        return
      }
      c.state = isApiError(e) && e.status === 409 ? 'conflict' : 'failed'
      c.error = e instanceof Error ? e.message : 'Could not send update'
      if (isApiError(e) && e.fields.current_state) {
        const current = JSON.parse(e.fields.current_state) as Task & {
          checklist_json?: string
        }
        c.server = {
          ...current,
          checklist: current.checklist_json
            ? (JSON.parse(current.checklist_json) as Task['checklist'])
            : current.checklist,
          updates: [],
        }
      }
      blocked.add(c.task)
    }
    await write('commands', c)
    changed()
  }
}
/** Explicit user choice: keep the server record and discard this task's unapplied local commands. */
export async function discardTask(user: number, task: number) {
  const server = await api.get<Task>(`/api/field/tasks/${task}`)
  const rows = await commands(user)
  const db = await database()
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(['commands', 'tasks'], 'readwrite')
    for (const c of rows)
      if (c.task === task && c.state !== 'confirmed')
        tx.objectStore('commands').delete(c.id)
    tx.objectStore('tasks').put({ user, task: server }, `${user}:${task}`)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
  changed()
}
