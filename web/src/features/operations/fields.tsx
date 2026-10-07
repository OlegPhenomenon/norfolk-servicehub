import { useId } from 'react'
import { useQuery } from '@tanstack/react-query'
import type { FieldComponentProps } from '@/featureTypes'
import { api } from '@/api/client'
import {
  Field,
  TextInput,
  Textarea,
  RadioGroup,
  DateTime,
  ErrorAlert,
} from '@/ui'
import { IslandMap } from './IslandMap'
import { addDays, conditions, localInput, today, utcInput } from './time'
import type { Availability, Slot } from './types'

export function BookingSlot({
  field,
  value,
  onChange,
  error,
  disabled,
}: FieldComponentProps) {
  const group = useId()
  const v = (value ?? {}) as Partial<Slot>
  const start = v.start_at ? localInput(v.start_at) : `${today()}T18:00`
  const end = v.end_at ? localInput(v.end_at) : `${today()}T23:00`
  const date = start.slice(0, 10)
  const q = useQuery({
    queryKey: ['operations', 'availability', date],
    queryFn: () =>
      api.get<Availability>('/api/public/venues/rawson-hall/availability', {
        query: { from: date, to: addDays(date, 1) },
      }),
    staleTime: 15000,
  })
  const update = (patch: Partial<Slot>) =>
    onChange({
      unit_code: v.unit_code ?? 'rawson-main',
      start_at: v.start_at ?? utcInput(start),
      end_at: v.end_at ?? utcInput(end),
      attendees: v.attendees ?? 1,
      ...patch,
    })
  const unit = q.data?.units.find(
    (u) => u.code === (v.unit_code ?? 'rawson-main'),
  )
  const clashes = unit?.busy.filter(
    (b) =>
      Date.parse(b.start_at) <
        Date.parse(v.end_at ?? utcInput(end)) + unit.cleanup_minutes * 60000 &&
      Date.parse(b.end_at) >
        Date.parse(v.start_at ?? utcInput(start)) - unit.prep_minutes * 60000,
  )
  return (
    <fieldset className="space-y-4" disabled={disabled}>
      <legend className="font-semibold">{field.label}</legend>
      <RadioGroup
        legend="Space"
        name={group}
        required
        value={v.unit_code ?? 'rawson-main'}
        onChange={(code) => update({ unit_code: code })}
        error={error}
        options={(
          q.data?.units ?? [
            {
              code: 'rawson-main',
              name: 'Main hall',
              capacity: null,
              active: true,
            },
            {
              code: 'rawson-supper',
              name: 'Supper room',
              capacity: null,
              active: true,
            },
            {
              code: 'rawson-whole',
              name: 'Whole venue',
              capacity: null,
              active: true,
            },
          ]
        ).map((u) => ({
          value: u.code,
          label: u.name,
          hint: u.capacity
            ? `Up to ${u.capacity} guests`
            : 'Capacity: confirm with Council',
          disabled: !u.active,
        }))}
      />
      <Field label="Date" required>
        <TextInput
          type="date"
          min={today()}
          value={date}
          onChange={(e) => {
            if (e.target.value)
              update({
                start_at: utcInput(`${e.target.value}T${start.slice(11)}`),
                end_at: utcInput(`${e.target.value}T${end.slice(11)}`),
              })
          }}
        />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Start (Norfolk time)" required>
          <TextInput
            type="time"
            value={start.slice(11)}
            onChange={(e) => {
              if (e.target.value)
                update({ start_at: utcInput(`${date}T${e.target.value}`) })
            }}
          />
        </Field>
        <Field
          label="End (Norfolk time)"
          required
          hint="Use 00:00 for midnight"
        >
          <TextInput
            type="time"
            value={end.slice(11)}
            onChange={(e) => {
              if (e.target.value)
                update({
                  end_at: utcInput(
                    `${e.target.value === '00:00' ? addDays(date, 1) : date}T${e.target.value}`,
                  ),
                })
            }}
          />
        </Field>
      </div>
      <Field label="Number of guests" required>
        <TextInput
          type="number"
          min={1}
          value={v.attendees ?? ''}
          onChange={(e) => update({ attendees: Number(e.target.value) })}
        />
      </Field>
      <p className="text-sm text-muted">
        Hire hours: 07:00–midnight; at most 12 hours. {conditions}
      </p>
      <div aria-live="polite" className="rounded-lg border border-line p-3">
        {q.isPending ? (
          'Checking availability…'
        ) : q.error ? (
          <ErrorAlert error={q.error} />
        ) : !unit?.active ? (
          'This space is unavailable.'
        ) : clashes?.length ? (
          'Unavailable for your selected times, including preparation and cleanup. Choose another time or room.'
        ) : (
          'No confirmed booking overlaps your selected times. Council will confirm your request.'
        )}
        <div
          className="mt-3 flex h-7 overflow-hidden rounded border border-line"
          role="img"
          aria-label="Availability from 7 am to midnight. Busy intervals are listed below."
        >
          {Array.from({ length: 17 }, (_, n) => {
            const hour = String(n + 7).padStart(2, '0')
            const a = Date.parse(utcInput(`${date}T${hour}:00`))
            const b =
              n === 16
                ? Date.parse(utcInput(`${addDays(date, 1)}T00:00`))
                : a + 3600000
            const busy = unit?.busy.some(
              (block) =>
                Date.parse(block.start_at) < b && Date.parse(block.end_at) > a,
            )
            return (
              <span
                key={n}
                className={`flex-1 border-r border-line ${busy ? 'bg-warning' : 'bg-pine/20'}`}
                title={`${hour}:00: ${busy ? 'Booked or unavailable' : 'Free'}`}
              />
            )
          })}
        </div>
        <div className="flex justify-between text-xs text-muted">
          <span>07:00</span>
          <span>24:00</span>
        </div>
        <div className="mt-2 space-y-1">
          {unit?.busy.map((b, i) => (
            <p
              key={i}
              className="border-l-4 border-warning bg-warning-50 p-2 text-sm"
            >
              {b.label}: <DateTime value={b.start_at} format="time" />–
              <DateTime value={b.end_at} format="time" /> (includes buffers)
            </p>
          ))}
        </div>
      </div>
    </fieldset>
  )
}
export function EquipmentRequest({
  field,
  value,
  onChange,
  error,
  disabled,
}: FieldComponentProps) {
  const v = (value ?? {}) as {
    description?: string
    requested_hours?: number
    preferred_date?: string
    site_text?: string
  }
  const update = (patch: typeof v) =>
    onChange({
      description: '',
      requested_hours: 4,
      preferred_date: today(),
      site_text: '',
      ...v,
      ...patch,
    })
  return (
    <fieldset disabled={disabled} className="space-y-4">
      <legend className="font-semibold">{field.label}</legend>
      <Field label="Plant needed and purpose of work" required error={error}>
        <Textarea
          value={v.description ?? ''}
          onChange={(e) => update({ description: e.target.value })}
          maxLength={2000}
        />
      </Field>
      <Field
        label="Requested hours (estimate)"
        required
        hint="Final charge uses approved job-card time"
      >
        <TextInput
          type="number"
          min={1}
          max={240}
          value={v.requested_hours ?? 4}
          onChange={(e) => update({ requested_hours: Number(e.target.value) })}
        />
      </Field>
      <Field label="Preferred date" required>
        <TextInput
          type="date"
          min={today()}
          value={v.preferred_date ?? today()}
          onChange={(e) => update({ preferred_date: e.target.value })}
        />
      </Field>
      <Field label="Where plant is required (road, property or area)" required>
        <TextInput
          value={v.site_text ?? ''}
          onChange={(e) => update({ site_text: e.target.value })}
          maxLength={500}
        />
      </Field>
      <p className="text-sm text-muted">
        Council plant includes fuel, oil and the operator’s ordinary-time wages.
        Job-card times run from departure from the Local Services Depot until
        return. Notify accidents or damage immediately.
      </p>
    </fieldset>
  )
}
export function Location({
  field,
  value,
  onChange,
  error,
  disabled,
}: FieldComponentProps) {
  const v = (value ?? {}) as {
    lat?: number
    lng?: number
    description?: string
  }
  const update = (patch: typeof v) =>
    onChange({ lat: -29.04, lng: 167.95, description: '', ...v, ...patch })
  return (
    <fieldset disabled={disabled} className="space-y-4">
      <legend className="font-semibold">{field.label}</legend>
      <p>Click the map to place a pin, or enter coordinates below.</p>
      <IslandMap
        points={
          v.lat !== undefined && v.lng !== undefined
            ? [{ lat: v.lat, lng: v.lng, label: 'Selected location' }]
            : []
        }
        onPin={disabled ? undefined : (lat, lng) => update({ lat, lng })}
      />
      <div className="grid grid-cols-2 gap-3">
        <Field label="Latitude" required error={error}>
          <TextInput
            type="number"
            step="any"
            min={-29.15}
            max={-28.98}
            value={v.lat ?? ''}
            onChange={(e) => update({ lat: Number(e.target.value) })}
          />
        </Field>
        <Field label="Longitude" required>
          <TextInput
            type="number"
            step="any"
            min={167.9}
            max={168.01}
            value={v.lng ?? ''}
            onChange={(e) => update({ lng: Number(e.target.value) })}
          />
        </Field>
      </div>
      <Field
        label="Location description"
        required
        hint="Help the field worker find the issue"
      >
        <Textarea
          maxLength={500}
          value={v.description ?? ''}
          onChange={(e) => update({ description: e.target.value })}
        />
      </Field>
    </fieldset>
  )
}
