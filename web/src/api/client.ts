/**
 * The one way to call the backend. Never use `fetch` directly in features.
 *
 *   const cases = await api.get<CaseSummary[]>('/api/cases', { query: { status: 'submitted' } })
 *   await api.post('/api/cases/12/messages', { body: 'Thanks' })
 *   await api.post('/api/cases/12/submit', body, { idempotencyKey: key })   // key from newIdempotencyKey()
 *   await api.upload<BlobRow>('/api/blobs', { file, fields: { kind: 'plan' } })
 *
 * - Adds `X-CSRF-Token` to every non-GET request (token comes from `GET /api/me`).
 * - Sends cookies (`credentials: 'same-origin'`).
 * - Throws `ApiError` for any non-2xx response, parsed from the error envelope
 *   `{"error": {"code", "message", "fields"}}`. Network failures throw `ApiError` with status 0, code `network`.
 * - Resolves `undefined` for 204 / empty bodies.
 */

/** Error codes from ARCHITECTURE §3, plus client-side `network` / `gone`. */
export type ApiErrorCode =
  | 'unauthorized'
  | 'mfa_required'
  | 'forbidden'
  | 'not_found'
  | 'conflict'
  | 'stale_revision'
  | 'idempotency_mismatch'
  | 'validation'
  | 'rate_limited'
  | 'internal'
  | 'gone'
  | 'network'

export class ApiError extends Error {
  readonly status: number
  /** One of `ApiErrorCode`; unknown server codes are passed through as-is. */
  readonly code: ApiErrorCode | (string & {})
  /** Per-field validation messages keyed by field name (empty object when none). */
  readonly fields: Record<string, string>

  constructor(status: number, code: string, message: string, fields: Record<string, string> = {}) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.fields = fields
  }
}

/** Type guard; optionally checks the code: `isApiError(e, 'stale_revision')`. */
export function isApiError(e: unknown, code?: ApiErrorCode): e is ApiError {
  return e instanceof ApiError && (code === undefined || e.code === code)
}

const FALLBACK: Record<number, [ApiErrorCode, string]> = {
  400: ['validation', 'The request was not valid.'],
  401: ['unauthorized', 'Please sign in to continue.'],
  403: ['forbidden', 'You do not have access to this.'],
  404: ['not_found', 'We could not find what you were looking for.'],
  409: ['conflict', 'This changed while you were working. Reload and try again.'],
  410: ['gone', 'This demonstration has ended.'],
  413: ['validation', 'The file is too large.'],
  422: ['validation', 'Please check the highlighted fields.'],
  429: ['rate_limited', 'Too many attempts. Please wait a few minutes and try again.'],
}

/**
 * Turn a failed response (status + raw body text) into an `ApiError`.
 * Accepts the standard envelope; anything else falls back to a friendly message for the status.
 */
export function parseApiError(status: number, bodyText: string): ApiError {
  const [fallbackCode, fallbackMessage] = FALLBACK[status] ?? [
    'internal',
    'Something went wrong on our side. Please try again.',
  ]
  // Envelope as sent by the server; every property is still checked with typeof below.
  interface Envelope {
    error?: { code?: unknown; message?: unknown; fields?: unknown } | null
  }
  let parsed: Envelope | null = null
  try {
    parsed = bodyText ? (JSON.parse(bodyText) as Envelope) : null
  } catch {
    parsed = null
  }
  const err = parsed && typeof parsed === 'object' && parsed.error && typeof parsed.error === 'object' ? parsed.error : null
  if (!err) return new ApiError(status, fallbackCode, fallbackMessage)

  const code = typeof err.code === 'string' && err.code ? err.code : fallbackCode
  const message = typeof err.message === 'string' && err.message ? err.message : fallbackMessage
  const fields: Record<string, string> = {}
  if (err.fields && typeof err.fields === 'object' && !Array.isArray(err.fields)) {
    for (const [k, v] of Object.entries(err.fields as Record<string, unknown>)) {
      if (typeof v === 'string') fields[k] = v
      else if (Array.isArray(v) && typeof v[0] === 'string') fields[k] = v[0]
    }
  }
  return new ApiError(status, code, message, fields)
}

/** A fresh key for the `Idempotency-Key` header. Create it once per user intent (e.g. when the form mounts). */
export function newIdempotencyKey(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
}

// ---------------------------------------------------------------------------
// CSRF token (kept in sync by `useMe()`)
// ---------------------------------------------------------------------------

let csrfToken: string | null = null
let csrfLoaded = false

/** Called by `useMe()` whenever `/api/me` is (re)loaded. */
export function setCsrfToken(token: string | null): void {
  csrfToken = token
  csrfLoaded = true
}

async function ensureCsrfToken(): Promise<string | null> {
  if (csrfLoaded) return csrfToken
  try {
    const res = await fetch('/api/me', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
    if (res.ok) {
      const me = (await res.json()) as { csrf_token?: string | null }
      setCsrfToken(me.csrf_token ?? null)
    }
  } catch {
    // The real request below will surface the network error.
  }
  return csrfToken
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

export type QueryValue = string | number | boolean | null | undefined

export interface RequestOptions {
  /** Appended as a query string; null/undefined values are skipped. */
  query?: Record<string, QueryValue | QueryValue[]>
  /** Sent as `Idempotency-Key` (see `newIdempotencyKey`). */
  idempotencyKey?: string
  headers?: Record<string, string>
  signal?: AbortSignal
}

type Method = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

export function buildUrl(path: string, query?: RequestOptions['query']): string {
  if (!query) return path
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(query)) {
    const values = Array.isArray(value) ? value : [value]
    for (const v of values) if (v !== null && v !== undefined) params.append(key, String(v))
  }
  const qs = params.toString()
  return qs ? `${path}${path.includes('?') ? '&' : '?'}${qs}` : path
}

async function request<T>(method: Method, path: string, body: BodyInit | undefined, isJson: boolean, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json', ...opts.headers }
  if (isJson) headers['Content-Type'] = 'application/json'
  if (opts.idempotencyKey) headers['Idempotency-Key'] = opts.idempotencyKey
  if (method !== 'GET') {
    const token = await ensureCsrfToken()
    if (token) headers['X-CSRF-Token'] = token
  }

  let res: Response
  try {
    res = await fetch(buildUrl(path, opts.query), {
      method,
      headers,
      body,
      credentials: 'same-origin',
      signal: opts.signal,
    })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') throw e
    throw new ApiError(0, 'network', 'We could not reach the server. Check your connection and try again.')
  }

  const text = await res.text()
  if (!res.ok) throw parseApiError(res.status, text)
  if (res.status === 204 || text === '') return undefined as T
  const type = res.headers.get('Content-Type') ?? ''
  return (type.includes('json') ? JSON.parse(text) : text) as T
}

function jsonBody(body: unknown): BodyInit | undefined {
  return body === undefined ? undefined : JSON.stringify(body)
}

export interface UploadInput {
  file: File | Blob
  /** Form field name for the file (default `file`). */
  fileField?: string
  /** File name to send when `file` is a Blob. */
  fileName?: string
  /** Extra text fields. */
  fields?: Record<string, string | number | boolean>
}

export const api = {
  get: <T>(path: string, opts?: RequestOptions) => request<T>('GET', path, undefined, false, opts),
  post: <T = void>(path: string, body?: unknown, opts?: RequestOptions) => request<T>('POST', path, jsonBody(body), body !== undefined, opts),
  put: <T = void>(path: string, body?: unknown, opts?: RequestOptions) => request<T>('PUT', path, jsonBody(body), body !== undefined, opts),
  patch: <T = void>(path: string, body?: unknown, opts?: RequestOptions) => request<T>('PATCH', path, jsonBody(body), body !== undefined, opts),
  delete: <T = void>(path: string, opts?: RequestOptions) => request<T>('DELETE', path, undefined, false, opts),
  /** multipart/form-data POST (the browser sets the boundary header). Pass a `FormData` or an `UploadInput`. */
  upload: <T>(path: string, input: FormData | UploadInput, opts?: RequestOptions) => {
    let form: FormData
    if (input instanceof FormData) {
      form = input
    } else {
      form = new FormData()
      for (const [k, v] of Object.entries(input.fields ?? {})) form.append(k, String(v))
      const name = input.fileName ?? (input.file instanceof File ? input.file.name : 'upload')
      form.append(input.fileField ?? 'file', input.file, name)
    }
    return request<T>('POST', path, form, false, opts)
  },
}
