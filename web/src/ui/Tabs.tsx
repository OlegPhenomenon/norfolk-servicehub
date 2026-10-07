import { useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { cn } from './cn'

export interface TabItem {
  key: string
  label: ReactNode
  /** Small count bubble after the label. */
  count?: number
  /** Rendered only while active. */
  content: ReactNode
}

export interface TabsProps {
  tabs: TabItem[]
  /** Controlled active key (e.g. synced with `?tab=` in the URL). */
  value?: string
  onChange?: (key: string) => void
  /** Initial key when uncontrolled (defaults to the first tab). */
  defaultValue?: string
  /** Accessible name of the tab list. */
  label: string
  className?: string
}

/**
 * Accessible tabs (arrow keys / Home / End move between tabs).
 *   <Tabs label="Case sections" tabs={[{ key: 'overview', label: 'Overview', content: <Overview /> }]} />
 */
export function Tabs({ tabs, value, onChange, defaultValue, label, className }: TabsProps) {
  const baseId = useId()
  const [internal, setInternal] = useState(defaultValue ?? tabs[0]?.key ?? '')
  const active = value ?? internal
  const current = tabs.find((t) => t.key === active) ?? tabs[0]
  const refs = useRef<Map<string, HTMLButtonElement>>(new Map())

  const select = (key: string) => {
    if (value === undefined) setInternal(key)
    onChange?.(key)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next = -1
    if (e.key === 'ArrowRight') next = (index + 1) % tabs.length
    else if (e.key === 'ArrowLeft') next = (index - 1 + tabs.length) % tabs.length
    else if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = tabs.length - 1
    const tab = tabs[next]
    if (!tab) return
    e.preventDefault()
    select(tab.key)
    refs.current.get(tab.key)?.focus()
  }

  return (
    <div className={className}>
      <div role="tablist" aria-label={label} className="flex gap-1 overflow-x-auto border-b border-line">
        {tabs.map((tab, i) => {
          const selected = tab.key === current?.key
          return (
            <button
              key={tab.key}
              ref={(el) => {
                if (el) refs.current.set(tab.key, el)
                else refs.current.delete(tab.key)
              }}
              type="button"
              role="tab"
              id={`${baseId}-tab-${tab.key}`}
              aria-selected={selected}
              aria-controls={`${baseId}-panel-${tab.key}`}
              tabIndex={selected ? 0 : -1}
              onClick={() => select(tab.key)}
              onKeyDown={(e) => onKeyDown(e, i)}
              className={cn(
                '-mb-px inline-flex min-h-11 items-center gap-2 whitespace-nowrap border-b-[3px] px-4 font-semibold transition-colors',
                selected ? 'border-primary text-primary' : 'border-transparent text-muted hover:text-ink hover:border-line-strong',
              )}
            >
              {tab.label}
              {tab.count !== undefined ? (
                <span className={cn('rounded-full px-2 text-xs leading-5', selected ? 'bg-primary text-white' : 'bg-sunken text-muted')}>{tab.count}</span>
              ) : null}
            </button>
          )
        })}
      </div>
      {current ? (
        <div role="tabpanel" id={`${baseId}-panel-${current.key}`} aria-labelledby={`${baseId}-tab-${current.key}`} tabIndex={0} className="pt-6 focus-visible:outline-offset-4">
          {current.content}
        </div>
      ) : null}
    </div>
  )
}
