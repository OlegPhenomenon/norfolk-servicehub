const parts = (utc: string | number) =>
  new Intl.DateTimeFormat('en-CA', {
    timeZone: 'Pacific/Norfolk',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(new Date(utc))
export function localInput(utc: string | number): string {
  const p = Object.fromEntries(parts(utc).map((x) => [x.type, x.value]))
  return `${p.year}-${p.month}-${p.day}T${p.hour}:${p.minute}`
}
/** Interpret a datetime-local as Norfolk wall time, independent of the device timezone. */
export function utcInput(wall: string): string {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(wall)) return ''
  const target = Date.parse(`${wall}:00Z`)
  let guess = target
  for (let i = 0; i < 3; i++)
    guess += target - Date.parse(`${localInput(guess)}:00Z`)
  return new Date(guess).toISOString()
}
export function addDays(date: string, days: number): string {
  return new Date(Date.parse(`${date}T12:00:00Z`) + days * 86400000)
    .toISOString()
    .slice(0, 10)
}
export const today = () => localInput(Date.now()).slice(0, 10)
export const priceNote = 'FY2026-27 schedule (demo copy — confirm with Council)'
export const conditions =
  'Loud music stops by 10 pm unless agreed. Return keys and remove property by noon on the next business day. Public liability insurance: $20 million, or Council agreement for casual hirers. Cancellation notice: more than 7 days for meetings; 30 days for weddings, concerts, stage shows and balls.'
