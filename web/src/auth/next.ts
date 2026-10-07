/** Read a safe in-app `?next=` path (rejects absolute and protocol-relative URLs). */
export function safeNext(search: string, fallback: string): string {
  const next = new URLSearchParams(search).get('next')
  return next && next.startsWith('/') && !next.startsWith('//') ? next : fallback
}

/** `/login?next=/staff/cases/3` */
export function withNext(path: string, next: string): string {
  return `${path}?next=${encodeURIComponent(next)}`
}
