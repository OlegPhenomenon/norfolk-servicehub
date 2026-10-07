/** Short public copy keeps the opening sentence; decimal prices are not sentence boundaries. */
export function firstSentence(text: string, limit?: number): string {
  const sentence = text.trim().split(/(?<=[.!?])\s+/)[0] ?? ''
  if (!limit || sentence.length <= limit) return sentence
  const clipped = sentence.slice(0, limit - 1).replace(/\s+\S*$/, '').trimEnd()
  return `${clipped}…`
}

export const DEMO_SCHEDULE_NOTE = 'Prices are a demo copy of the FY2026-27 schedule; time targets are illustrative'
