import { useCallback, useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMe } from '@/auth/useMe'
import { commands, subscribe, sync } from './outbox'
export function useOutbox() {
  const { data: me } = useMe()
  const user = me?.user?.id ?? 0
  const qc = useQueryClient()
  const [offline, setOffline] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const q = useQuery({
    queryKey: ['operations', 'outbox', user],
    queryFn: () => commands(user),
    enabled: !!user,
    // This query reads IndexedDB; losing the network must not pause device-save feedback.
    networkMode: 'always',
  })
  const send = useCallback(() => {
    void sync(user, offline)
      .then(() => qc.invalidateQueries({ queryKey: ['operations'] }))
      .catch(setError)
  }, [user, offline, qc])
  useEffect(() => {
    if (!user) return
    const unsubscribe = subscribe(() => {
      void qc.invalidateQueries({ queryKey: ['operations'] })
    })
    window.addEventListener('online', send)
    send()
    return () => {
      unsubscribe()
      window.removeEventListener('online', send)
    }
  }, [user, send, qc])
  return { user, offline, setOffline, send, commands: q.data ?? [], error }
}
