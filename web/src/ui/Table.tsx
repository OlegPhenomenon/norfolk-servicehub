import type { ReactNode } from 'react'
import { cn } from './cn'

export interface Column<T> {
  /** Stable key for React. */
  key: string
  header: ReactNode
  /** Cell content. Put a <Link> in the main column to make rows navigable. */
  cell: (row: T) => ReactNode
  align?: 'left' | 'right' | 'center'
  /** Extra classes for th/td (e.g. `w-32`, `hidden md:table-cell`). */
  className?: string
}

export interface TableProps<T> {
  columns: Column<T>[]
  rows: T[]
  rowKey: (row: T) => string | number
  /** Accessible table name (shown visually unless `hideCaption`). */
  caption: string
  hideCaption?: boolean
  /** Shown instead of the table body when `rows` is empty. */
  empty?: ReactNode
  dense?: boolean
  /** Highlight a row (e.g. overdue items). */
  rowClassName?: (row: T) => string | undefined
  className?: string
}

const ALIGN = { left: 'text-left', right: 'text-right', center: 'text-center' } as const

/**
 * Data table with horizontal scrolling on narrow screens.
 *
 *   <Table caption="Open cases" rows={cases} rowKey={(c) => c.id} columns={[
 *     { key: 'number', header: 'Number', cell: (c) => <Link className="link" to={`/staff/cases/${c.id}`}>{c.number}</Link> },
 *     { key: 'status', header: 'Status', cell: (c) => <StatusPill status={c.status} /> },
 *     { key: 'due', header: 'Due', align: 'right', cell: (c) => <DateTime value={c.due_at} format="date" /> },
 *   ]} empty={<EmptyState title="No open cases" />} />
 */
export function Table<T>({ columns, rows, rowKey, caption, hideCaption = true, empty, dense, rowClassName, className }: TableProps<T>) {
  if (rows.length === 0 && empty) return <>{empty}</>
  const pad = dense ? 'px-3 py-2' : 'px-4 py-3'
  return (
    <div role="region" aria-label={caption} tabIndex={0} className={cn('overflow-x-auto rounded-[var(--radius-card)] border border-line bg-surface', className)}>
      <table className="w-full border-collapse text-[0.95rem]">
        <caption className={cn(hideCaption ? 'sr-only' : 'px-4 pt-4 pb-2 text-left font-semibold')}>{caption}</caption>
        <thead>
          <tr className="border-b border-line bg-sunken/70">
            {columns.map((col) => (
              <th key={col.key} scope="col" className={cn(pad, 'text-sm font-semibold text-muted whitespace-nowrap', ALIGN[col.align ?? 'left'], col.className)}>
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={rowKey(row)} className={cn('border-b border-line last:border-b-0 hover:bg-primary-50/40', rowClassName?.(row))}>
              {columns.map((col) => (
                <td key={col.key} className={cn(pad, 'align-top', ALIGN[col.align ?? 'left'], col.className)}>
                  {col.cell(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
