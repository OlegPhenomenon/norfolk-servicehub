import { useState, type FormEvent } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { useMutation } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Alert, Button, ButtonLink, ErrorAlert, Field, TextInput } from '@/ui'
import { AuthCard } from './AuthCard'
import { safeNext, withNext } from './next'
import { useMe, useRefreshMe } from './useMe'

/** `/login` — email + password. Staff continue to the TOTP step. */
export function LoginPage() {
  const { data: me } = useMe()
  const refreshMe = useRefreshMe()
  const navigate = useNavigate()
  const location = useLocation()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')

  const login = useMutation({
    mutationFn: () => api.post('/api/auth/login', { email, password }),
    onSuccess: async () => {
      const fresh = await refreshMe()
      const isStaff = fresh.user?.kind === 'staff'
      const next = safeNext(location.search, isStaff ? '/staff' : '/my')
      if (isStaff && fresh.mfa_required) {
        void navigate(withNext(fresh.user?.totp_enabled ? '/login/totp' : '/staff/settings/2fa', next), { replace: true })
      } else {
        void navigate(next, { replace: true })
      }
    },
  })

  const submit = (e: FormEvent) => {
    e.preventDefault()
    login.mutate()
  }
  const fields = isApiError(login.error) ? login.error.fields : {}

  return (
    <AuthCard
      title="Sign in"
      description="Follow your requests, reply to the council and pay online."
      aside={
        me?.demo_mode ? (
          <Alert
            tone="info"
            title="Exploring the demonstration?"
            actions={
              <ButtonLink to="/demo" variant="secondary" icon="users">
                Choose a demo persona
              </ButtonLink>
            }
          >
            Demo personas have no passwords. Pick a resident or a staff member and sign in with one click.
          </Alert>
        ) : undefined
      }
    >
      <form onSubmit={submit} noValidate className="flex flex-col gap-5">
        {login.error && !isApiError(login.error, 'validation') ? <ErrorAlert error={login.error} title="Could not sign you in" /> : null}
        <Field label="Email address" error={fields.email} required>
          <TextInput type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field label="Password" error={fields.password} required>
          <TextInput type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </Field>
        <Button type="submit" size="lg" loading={login.isPending} fullWidth>
          Sign in
        </Button>
        <p className="text-center text-muted">
          New here?{' '}
          <Link to="/register" className="link">
            Create a resident account
          </Link>
        </p>
      </form>
    </AuthCard>
  )
}
