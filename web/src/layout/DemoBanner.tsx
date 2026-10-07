import { Link } from 'react-router'
import { useMe } from '@/auth/useMe'
import { formatDateTime, Icon } from '@/ui'

/** Thin strip on every page in demo mode: fictional data + next reset time (Norfolk time). */
export function DemoBanner() {
  const { data: me } = useMe()
  if (!me?.demo_mode) return null
  const reset = me.next_reset_at ? formatDateTime(me.next_reset_at, 'time') : null
  return (
    <div className="bg-primary-900 text-[0.875rem] text-white/90">
      <div className="mx-auto flex max-w-7xl flex-wrap items-center gap-x-4 gap-y-1 px-4 py-2 sm:px-6">
        <p className="flex items-center gap-2">
          <Icon name="info" size={16} className="text-focus" />
          <span>
            <strong className="font-semibold text-white">Demonstration with fictional data</strong>
            {reset ? <> — resets at {reset} (Norfolk Island time)</> : null}
          </span>
        </p>
        <nav aria-label="Demo tools" className="flex gap-4 sm:ml-auto">
          <Link to="/demo" className="underline decoration-white/40 underline-offset-2 hover:decoration-white">
            Switch persona
          </Link>
          <Link to="/mock/mail" className="underline decoration-white/40 underline-offset-2 hover:decoration-white">
            DemoMail
          </Link>
        </nav>
      </div>
    </div>
  )
}
