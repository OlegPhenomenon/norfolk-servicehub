import { describe, it, expect } from 'vitest'
import { localInput, utcInput } from './time'
import { optimistic } from './outbox'
import type { Task } from './types'
describe('Norfolk wall time', () => {
  it('uses the island timezone even on devices elsewhere and handles summer/winter', () => {
    expect(utcInput('2026-11-14T18:00')).toBe('2026-11-14T06:00:00.000Z')
    expect(utcInput('2026-07-01T18:00')).toBe('2026-07-01T07:00:00.000Z')
    expect(localInput('2026-11-14T12:00:00Z')).toBe('2026-11-15T00:00')
    expect(utcInput('')).toBe('')
  })
})
describe('Offline projection', () => {
  const task = {
    id: 1,
    revision: 4,
    status: 'open',
    result_text: null,
    checklist: [{ key: 'safety', label: 'Safety checked', done: false }],
  } as Task
  it('preserves the original while applying the next expected revision', () => {
    const result = optimistic(task, { kind: 'result', body: 'Site inspected' })
    const checklist = optimistic(result, {
      kind: 'checklist',
      body: '{"key":"safety","done":true}',
    })
    const done = optimistic(checklist, { kind: 'status', body: 'done' })
    expect(task.revision).toBe(4)
    expect(task.checklist[0]?.done).toBe(false)
    expect(done.revision).toBe(7)
    expect(done.status).toBe('done')
    expect(done.result_text).toBe('Site inspected')
    expect(done.checklist[0]?.done).toBe(true)
  })
  it('a job card does not invent a task revision', () => {
    expect(
      optimistic(task, {
        started_at: '2026-10-07T00:00:00Z',
        ended_at: '2026-10-07T01:00:00Z',
      }).revision,
    ).toBe(4)
  })
})
