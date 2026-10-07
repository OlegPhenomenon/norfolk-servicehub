import { useState, type FormEvent } from 'react'
import { Link, useNavigate } from 'react-router'
import { useMutation } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Alert, Button, ErrorAlert, Field, TextInput, useToast } from '@/ui'
import { AuthCard } from './AuthCard'
import { useMe, useRefreshMe } from './useMe'

/** `/register` — resident self-registration. */
export function RegisterPage() {
  const { data: me } = useMe()
  const refreshMe = useRefreshMe()
  const navigate = useNavigate()
  const toast = useToast()
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')

  const register = useMutation({
    mutationFn: () => api.post('/api/auth/register', { name, email, password }),
    onSuccess: async () => {
      const fresh = await refreshMe()
      if (fresh.user) {
        toast.success('Your account is ready.', `Welcome, ${fresh.user.name}`)
        void navigate('/my', { replace: true })
      } else {
        toast.success('Your account is ready. Please sign in.')
        void navigate('/login', { replace: true })
      }
    },
  })

  const submit = (e: FormEvent) => {
    e.preventDefault()
    register.mutate()
  }
  const fields = isApiError(register.error) ? register.error.fields : {}

  return (
    <AuthCard title="Create an account" description="One account for all your council requests — hall hire, approvals, certificates, reports.">
      <form onSubmit={submit} noValidate className="flex flex-col gap-5">
        {me?.demo_mode ? (
          <Alert tone="warning" title="Demonstration site">
            Do not use a real password or personal details. Accounts are deleted at the next reset.
          </Alert>
        ) : null}
        {register.error && !isApiError(register.error, 'validation') ? <ErrorAlert error={register.error} title="Could not create the account" /> : null}
        <Field label="Full name" error={fields.name} required>
          <TextInput autoComplete="name" value={name} onChange={(e) => setName(e.target.value)} />
        </Field>
        <Field label="Email address" hint="We send updates about your requests here." error={fields.email} required>
          <TextInput type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field label="Password" hint="At least 10 characters." error={fields.password} required>
          <TextInput type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </Field>
        <Button type="submit" size="lg" loading={register.isPending} fullWidth>
          Create account
        </Button>
        <p className="text-center text-muted">
          Already registered?{' '}
          <Link to="/login" className="link">
            Sign in
          </Link>
        </p>
      </form>
    </AuthCard>
  )
}
