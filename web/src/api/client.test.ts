import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, api, buildUrl, isApiError, parseApiError, setCsrfToken } from './client'

describe('parseApiError', () => {
  it('reads the standard error envelope', () => {
    const body = JSON.stringify({
      error: { code: 'validation', message: 'Please fix 2 fields.', fields: { email: 'Enter an email address', name: ['Required'] } },
    })
    const err = parseApiError(422, body)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(422)
    expect(err.code).toBe('validation')
    expect(err.message).toBe('Please fix 2 fields.')
    expect(err.fields).toEqual({ email: 'Enter an email address', name: 'Required' })
  })

  it('keeps distinct 401 and 409 codes', () => {
    expect(parseApiError(401, '{"error":{"code":"mfa_required","message":"Enter your code"}}').code).toBe('mfa_required')
    expect(parseApiError(409, '{"error":{"code":"stale_revision","message":"Changed"}}').code).toBe('stale_revision')
  })

  it('falls back to a friendly message for non-JSON bodies', () => {
    const err = parseApiError(502, '<html>Bad gateway</html>')
    expect(err.code).toBe('internal')
    expect(err.message).toMatch(/something went wrong/i)
    expect(err.fields).toEqual({})
  })

  it('falls back per status when the envelope is missing parts', () => {
    expect(parseApiError(404, '{}').code).toBe('not_found')
    expect(parseApiError(429, '').code).toBe('rate_limited')
    const partial = parseApiError(403, '{"error":{"message":"Nope"}}')
    expect(partial.code).toBe('forbidden')
    expect(partial.message).toBe('Nope')
  })

  it('isApiError narrows and checks the code', () => {
    const err = parseApiError(409, '{"error":{"code":"conflict","message":"Taken"}}')
    expect(isApiError(err)).toBe(true)
    expect(isApiError(err, 'conflict')).toBe(true)
    expect(isApiError(err, 'stale_revision')).toBe(false)
    expect(isApiError(new Error('x'))).toBe(false)
  })
})

describe('api requests', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('sends CSRF + idempotency headers and throws ApiError on failure', async () => {
    setCsrfToken('tok-123')
    const fetchMock = vi.fn(async () =>
      new Response('{"error":{"code":"idempotency_mismatch","message":"Different request"}}', {
        status: 409,
        headers: { 'Content-Type': 'application/json' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const promise = api.post('/api/cases/1/submit', { a: 1 }, { idempotencyKey: 'key-1' })
    await expect(promise).rejects.toMatchObject({ status: 409, code: 'idempotency_mismatch', message: 'Different request' })

    const init = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1]
    const headers = init.headers as Record<string, string>
    expect(headers['X-CSRF-Token']).toBe('tok-123')
    expect(headers['Idempotency-Key']).toBe('key-1')
    expect(headers['Content-Type']).toBe('application/json')
  })

  it('maps network failures to code "network"', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => Promise.reject(new TypeError('Failed to fetch'))))
    await expect(api.get('/api/me')).rejects.toMatchObject({ status: 0, code: 'network' })
  })

  it('resolves undefined for 204', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(null, { status: 204 })))
    await expect(api.post('/api/auth/logout')).resolves.toBeUndefined()
  })

  it('builds query strings, skipping empty values', () => {
    expect(buildUrl('/api/cases', { status: 'open', page: 2, q: undefined, tag: ['a', 'b'] })).toBe('/api/cases?status=open&page=2&tag=a&tag=b')
  })
})
