import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router'
import { api } from '@/api/client'
import type { Notification, NotificationList } from '@/api/types'
import { cn, DateTime, Icon, Spinner } from '@/ui'
import { usePopover } from './usePopover'

const NOTIFICATIONS_QUERY_KEY = ['notifications'] as const

/** Bell with unread count; dropdown lists recent in-app notifications. Polls every 30 s. */
export function NotificationBell({ inverted }: { inverted?: boolean }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const { open, setOpen, containerRef, triggerRef } = usePopover()
  const query = useQuery({
    queryKey: NOTIFICATIONS_QUERY_KEY,
    queryFn: () => api.get<NotificationList>('/api/notifications'),
    refetchInterval: 30_000,
  })
  const markRead = useMutation({
    mutationFn: (id: number) => api.post(`/api/notifications/${id}/read`),
    onSettled: () => qc.invalidateQueries({ queryKey: NOTIFICATIONS_QUERY_KEY }),
  })
  const markAll = useMutation({
    mutationFn: () => api.post('/api/notifications/read-all'),
    onSettled: () => qc.invalidateQueries({ queryKey: NOTIFICATIONS_QUERY_KEY }),
  })

  const unread = query.data?.unread_count ?? 0
  const items = query.data?.items ?? []

  const openItem = (n: Notification) => {
    if (!n.read_at) markRead.mutate(n.id)
    setOpen(false)
    if (n.link) void navigate(n.link)
  }

  return (
    <div ref={containerRef} className="relative">
      <button
        ref={triggerRef}
        type="button"
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => setOpen(!open)}
        className={cn('relative flex size-11 items-center justify-center rounded-lg', inverted ? 'text-white hover:bg-white/10' : 'text-ink hover:bg-sunken')}
      >
        <Icon name="bell" size={22} className="-ml-1" title={unread ? `Notifications, ${unread} unread` : 'Notifications'} />
        {unread > 0 ? (
          <span aria-hidden="true" className="absolute top-1 right-0.5 flex h-[1.125rem] min-w-[1.125rem] items-center justify-center rounded-full bg-danger px-1 text-[0.65rem] font-bold text-white ring-2 ring-surface">
            {unread > 9 ? '9+' : unread}
          </span>
        ) : null}
      </button>
      {open ? (
        <div className="absolute right-0 z-40 mt-2 w-[min(24rem,calc(100vw-2rem))] overflow-hidden rounded-xl border border-line bg-surface text-ink shadow-[var(--shadow-raised)]">
          <div className="flex items-center justify-between border-b border-line px-4 py-3">
            <h2 className="font-semibold">Notifications</h2>
            {unread > 0 ? (
              <button type="button" onClick={() => markAll.mutate()} className="link text-sm">
                Mark all as read
              </button>
            ) : null}
          </div>
          {query.isPending ? (
            <div className="flex justify-center py-8 text-primary">
              <Spinner />
            </div>
          ) : items.length === 0 ? (
            <p className="px-4 py-8 text-center text-muted">You have no notifications yet.</p>
          ) : (
            <ul className="max-h-[60vh] divide-y divide-line overflow-y-auto">
              {items.map((n) => (
                <li key={n.id}>
                  <button type="button" onClick={() => openItem(n)} className="flex w-full gap-3 px-4 py-3 text-left hover:bg-primary-50/60">
                    <span aria-hidden="true" className={cn('mt-2 size-2 shrink-0 rounded-full', n.read_at ? 'bg-transparent' : 'bg-primary')} />
                    <span className="min-w-0">
                      <span className={cn('block leading-snug', !n.read_at && 'font-semibold')}>
                        {n.subject}
                        {!n.read_at ? <span className="sr-only"> (unread)</span> : null}
                      </span>
                      <span className="mt-0.5 line-clamp-2 block text-sm text-muted">{n.body}</span>
                      <DateTime value={n.created_at} format="relative" className="mt-1 block text-xs text-subtle" />
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  )
}
