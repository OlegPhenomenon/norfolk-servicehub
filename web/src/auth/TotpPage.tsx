import { useState, type FormEvent } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router'
import { useMutation } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Button, ErrorAlert, Field, LoadingState, TextInput } from '@/ui'
import { AuthCard } from './AuthCard'
import { DemoAuthenticator } from './DemoAuthenticator'
import { safeNext, withNext } from './next'
import { useMe, useRefreshMe } from './useMe'

/** `/login/totp` — second step for staff. In demo mode the persona's live code is shown beside the input. */
export function TotpPage() {
  const { data: me, isPending } = useMe()
  const refreshMe = useRefreshMe()
  const navigate = useNavigate()
  const location = useLocation()
  const next = safeNext(location.search, '/staff')
  const [code, setCode] = useState('')

  const verify = useMutation({
    mutationFn: (value: string) => api.post('/api/auth/totp', { code: value }),
    onSuccess: async () => {
      await refreshMe()
      void navigate(next, { replace: true })
    },
  })

  if (isPending) return <LoadingState className="min-h-[50vh]" />
  if (!me?.user) return <Navigate to={withNext('/login', next)} replace />
  if (!me.mfa_required) return <Navigate to={next} replace />
  if (!me.user.totp_enabled) return <Navigate to={withNext('/staff/settings/2fa', next)} replace />

  const submit = (value: string) => {
    setCode(value)
    verify.mutate(value.replace(/\s+/g, ''))
  }
  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    submit(code)
  }
  const fieldError = isApiError(verify.error) ? (verify.error.fields.code ?? (isApiError(verify.error, 'validation') ? verify.error.message : undefined)) : undefined

  return (
    <AuthCard
      title="Enter your verification code"
      description={
        <>
          Signed in as <strong className="font-semibold text-ink">{me.user.name}</strong>. Open your authenticator app and enter the 6-digit code.
        </>
      }
      aside={me.demo_mode ? <DemoAuthenticator persona={me.user.persona_key} onUse={submit} /> : undefined}
    >
      <form onSubmit={onSubmit} noValidate className="flex flex-col gap-5">
        {verify.error && !fieldError ? <ErrorAlert error={verify.error} title="Code not accepted" /> : null}
        <Field label="6-digit code" error={fieldError} required>
          <TextInput
            inputMode="numeric"
            autoComplete="one-time-code"
            pattern="[0-9 ]*"
            maxLength={7}
            autoFocus
            value={code}
            onChange={(e) => setCode(e.target.value)}
            className="max-w-48 font-mono text-2xl tracking-[0.2em]"
          />
        </Field>
        <Button type="submit" size="lg" loading={verify.isPending} fullWidth>
          Verify and continue
        </Button>
      </form>
    </AuthCard>
  )
}
