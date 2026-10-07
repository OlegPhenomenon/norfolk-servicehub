import { ApiError } from '@/api/client'
/** Parse decimal input to integer cents without binary floating point. Server revalidates. */
export function cents(value: string, field = 'amount_cents'): number {
  if (!/^\d+(\.\d{1,2})?$/.test(value.trim())) throw new ApiError(422, 'validation', 'Please check the highlighted fields.', { [field]: 'Enter a positive amount with up to two decimal places.' })
  const [whole, fraction = ''] = value.trim().split('.')
  const amount = Number(whole) * 100 + Number(fraction.padEnd(2, '0'))
  if (!Number.isSafeInteger(amount)) throw new ApiError(422, 'validation', 'Amount is too large.', { [field]: 'Amount is too large.' })
  return amount
}
