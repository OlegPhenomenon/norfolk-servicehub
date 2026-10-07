import { useState, type FormEvent } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router'
import { useMutation } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import type { TotpEnrolment } from '@/api/types'
import { Alert, Button, ButtonLink, ErrorAlert, Field, LoadingState, Steps, TextInput, useToast } from '@/ui'
import { AuthCard } from './AuthCard'
import { safeNext, withNext } from './next'
import { useMe, useRefreshMe } from './useMe'

/** `/staff/settings/2fa` — set up TOTP for staff accounts that do not have it yet; shows status otherwise. */
export function TotpEnrolPage() {
  const { data: me, isPending } = useMe()
  const refreshMe = useRefreshMe()
  const navigate = useNavigate()
  const location = useLocation()
  const toast = useToast()
  const next = safeNext(location.search, '/staff')
  const [code, setCode] = useState('')

  const start = useMutation({ mutationFn: () => api.post<TotpEnrolment>('/api/auth/totp/enroll') })
  const confirm = useMutation({
    mutationFn: () => api.post('/api/auth/totp/enroll/confirm', { code: code.replace(/\s+/g, '') }),
    onSuccess: async () => {
      const fresh = await refreshMe()
      toast.success('Two-step verification is now on.')
      void navigate(fresh.mfa_required ? withNext('/login/totp', next) : next, { replace: true })
    },
  })

  if (isPending) return <LoadingState className="min-h-[50vh]" />
  if (!me?.user) return <Navigate to={withNext('/login', location.pathname)} replace />
  if (me.user.kind !== 'staff') return <Navigate to="/my" replace />

  if (me.user.totp_enabled) {
    return (
      <AuthCard title="Two-step verification" description="Staff accounts always need a code from an authenticator app after the password.">
        <Alert tone="success" title="Two-step verification is on">
          Your account is protected with an authenticator app. Ask the systems administrator to reset it if you lose your phone.
        </Alert>
        <div className="mt-6">
          {me.mfa_required ? <ButtonLink to={withNext('/login/totp', next)}>Enter your code</ButtonLink> : <ButtonLink to="/staff">Back to the staff workspace</ButtonLink>}
        </div>
      </AuthCard>
    )
  }

  const enrolment = start.data
  const fieldError = isApiError(confirm.error) ? (confirm.error.fields.code ?? (isApiError(confirm.error, 'validation') ? confirm.error.message : undefined)) : undefined
  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    confirm.mutate()
  }

  return (
    <AuthCard
      title="Set up two-step verification"
      description="Council staff sign in with a password and a code from an authenticator app (for example Microsoft Authenticator or Google Authenticator)."
      aside={
        <div className="rounded-2xl border border-line bg-surface p-6">
          <Steps
            label="Setup steps"
            current={!enrolment ? 'scan' : 'confirm'}
            steps={[
              { key: 'scan', label: 'Scan the QR code with your authenticator app' },
              { key: 'confirm', label: 'Enter the 6-digit code it shows' },
              { key: 'done', label: 'Sign in with a code from now on' },
            ]}
          />
        </div>
      }
    >
      {!enrolment ? (
        <div className="flex flex-col gap-4">
          {start.error ? <ErrorAlert error={start.error} /> : null}
          <Button size="lg" icon="key" loading={start.isPending} onClick={() => start.mutate()} fullWidth>
            Start setup
          </Button>
        </div>
      ) : (
        <form onSubmit={onSubmit} noValidate className="flex flex-col gap-5">
          <div className="flex flex-col items-center gap-3 rounded-xl border border-line bg-white p-4">
            <img src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(enrolment.qr_svg)}`} alt="QR code for your authenticator app" className="size-52" />
            <p className="text-center text-sm text-muted">
              Can’t scan? Enter this key manually:
              <code className="mt-1 block font-mono text-base tracking-wider break-all text-ink">{enrolment.secret}</code>
            </p>
          </div>
          {confirm.error && !fieldError ? <ErrorAlert error={confirm.error} /> : null}
          <Field label="6-digit code from the app" error={fieldError} required>
            <TextInput inputMode="numeric" autoComplete="one-time-code" maxLength={7} value={code} onChange={(e) => setCode(e.target.value)} className="max-w-48 font-mono text-2xl tracking-[0.2em]" />
          </Field>
          <Button type="submit" size="lg" loading={confirm.isPending} fullWidth>
            Turn on two-step verification
          </Button>
        </form>
      )}
    </AuthCard>
  )
}
