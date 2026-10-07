import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { ROLE_LABELS, type Role } from '@/api/types'
import { Alert, Button, Card, DateTime, EmptyState, ErrorAlert, Field, PageHeader, QueryView, Select, StatusPill, Table, TextInput, Textarea, DescriptionList } from '@/ui'
import { useMe } from '@/auth/useMe'
import type { StaffUser } from './types'
import { useCommand } from './useCommand'
export function UsersPage() {
  const q = useQuery({ queryKey: ['records', 'users'], queryFn: () => api.get<StaffUser[]>('/api/admin/users') })
  const [email, setEmail] = useState(''), [name, setName] = useState(''), [kind, setKind] = useState('resident'), [password, setPassword] = useState('')
  const create = useCommand('/api/admin/users', 'Account created')
  const fields = isApiError(create.error) ? create.error.fields : {}
  return <div className="space-y-6"><PageHeader title="Users" description="Manage resident and staff accounts. Staff enrol two-step verification before accessing their workspace." />
    <Card title="Create account"><form className="grid gap-4 sm:grid-cols-2" onSubmit={e => { e.preventDefault(); create.mutate({ email, display_name: name, kind, password }, { onSuccess: () => { setEmail(''); setName(''); setPassword('') } }) }}><ErrorAlert error={create.error} /><Field label="Full name" required error={fields.display_name}><TextInput value={name} onChange={e => setName(e.target.value)} /></Field><Field label="Email" required error={fields.email}><TextInput type="email" value={email} onChange={e => setEmail(e.target.value)} /></Field><Field label="Account type" required error={fields.kind}><Select value={kind} onChange={e => setKind(e.target.value)} options={[{ value: 'resident', label: 'Resident' }, { value: 'staff', label: 'Staff' }]} /></Field><Field label="Initial password" hint="At least 12 characters" required error={fields.password}><TextInput type="password" autoComplete="new-password" value={password} onChange={e => setPassword(e.target.value)} /></Field><Button type="submit" loading={create.isPending}>Create account</Button></form></Card>
    <QueryView query={q}>{users => <div className="space-y-4">{users.map(u => <UserCard key={u.id} user={u} />)}</div>}</QueryView></div>
}
function UserCard({ user }: { user: StaffUser }) {
  const { data: me } = useMe()
  const [role, setRole] = useState<Role>('intake')
  const grants = useQuery({ queryKey: ['records', 'roles', user.id], queryFn: () => api.get<{ id: number; role: Role; revoked_at: string | null }[]>(`/api/admin/users/${user.id}/roles`), enabled: user.kind === 'staff' })
  const grant = useCommand(`/api/admin/users/${user.id}/roles`, 'Role granted')
  const deactivate = useCommand(`/api/admin/users/${user.id}/deactivate`, 'Account deactivated')
  return <Card title={user.display_name} description={`${user.email} · ${user.kind} · ${user.is_active ? 'Active' : 'Inactive'}`}><ErrorAlert error={grant.error ?? deactivate.error} />
    {user.kind === 'staff' && <><QueryView query={grants}>{roles => <div className="flex flex-wrap gap-3">{roles.filter(g => !g.revoked_at).map(g => <RoleGrant key={g.id} userId={user.id} grant={g} />)}</div>}</QueryView>{me?.user?.id !== user.id && <form className="mt-4 flex flex-wrap items-end gap-4" onSubmit={e => { e.preventDefault(); grant.mutate({ role }) }}><Field label="Staff role" required><Select value={role} onChange={e => setRole(e.target.value as Role)} options={Object.entries(ROLE_LABELS).filter(([value]) => me?.roles.includes('manager') || !['manager', 'complaints_officer'].includes(value)).map(([value, label]) => ({ value, label }))} /></Field><Button type="submit" loading={grant.isPending}>Grant role</Button></form>}</>}
    <RecoveryControls user={user} />
    {user.is_active === 1 && <Button variant="danger-outline" className="mt-4" loading={deactivate.isPending} onClick={() => deactivate.mutate({})}>Deactivate account</Button>}
  </Card>
}
function RoleGrant({ userId, grant }: { userId: number; grant: { id: number; role: Role } }) {
  const revoke = useCommand(`/api/admin/users/${userId}/roles/${grant.id}/revoke`, 'Role revoked')
  return <div className="rounded-lg border border-line p-2"><span>{ROLE_LABELS[grant.role]}</span> <Button variant="ghost" loading={revoke.isPending} onClick={() => revoke.mutate({})}>Revoke</Button><ErrorAlert error={revoke.error} /></div>
}
const ADDRESSES = [{ key: 'notify.customer_care_email', label: 'Customer Care inbox' }, { key: 'notify.finance_email', label: 'Finance inbox' }, { key: 'notify.works_depot_email', label: 'Works Depot inbox' }]
export function SettingsPage() {
  const q = useQuery({ queryKey: ['records', 'settings'], queryFn: () => api.get<Record<string, string | number>>('/api/admin/settings') })
  return <div className="space-y-6"><PageHeader title="Notification settings" /><QueryView query={q}>{values => <SettingsForm key={JSON.stringify(values)} values={values} />}</QueryView></div>
}
function SettingsForm({ values }: { values: Record<string, string | number> }) {
  const [form, setForm] = useState(() => Object.fromEntries(ADDRESSES.map(a => [a.key, String(values[a.key] ?? '')])))
  const save = useCommand('/api/admin/settings', 'Notification addresses saved')
  const fields = isApiError(save.error) ? save.error.fields : {}
  return <Card title="Notification addresses"><form className="space-y-4" onSubmit={e => { e.preventDefault(); save.mutate(form) }}><ErrorAlert error={save.error} />{ADDRESSES.map(a => <Field key={a.key} label={a.label} error={fields[a.key]} required><TextInput type="email" value={form[a.key]} onChange={e => setForm({ ...form, [a.key]: e.target.value })} /></Field>)}<DescriptionList items={[{ label: 'Demo reset interval (read-only)', value: `${values['demo.reset_hours']} hours` }]} /><Button type="submit" loading={save.isPending}>Save addresses</Button></form></Card>
}
interface NotificationDelivery { id: number; channel: string; to_address: string; status: string; last_error: string | null; created_at: string; sent_at: string | null }
export function DeliveriesPage() {
  const q = useQuery({ queryKey: ['records', 'notifications'], queryFn: () => api.get<NotificationDelivery[]>('/api/admin/deliveries') })
  return <div className="space-y-6"><PageHeader title="Notification deliveries" description="Sent and failed email/SMS deliveries. Retrying preserves the original delivery log." /><QueryView query={q}>{d => <Table caption="Email and SMS" rows={d} rowKey={r => r.id} empty={<EmptyState title="No outbound notifications" />} columns={[{ key: 'to', header: 'Recipient', cell: r => r.to_address }, { key: 'channel', header: 'Channel', cell: r => r.channel }, { key: 'status', header: 'Status', cell: r => <StatusPill status={r.status} /> }, { key: 'error', header: 'Error', cell: r => r.last_error ?? '—' }, { key: 'at', header: 'Created', cell: r => <DateTime value={r.created_at} /> }, { key: 'retry', header: 'Action', cell: r => r.status === 'failed' ? <RetryNotification id={r.id} /> : '—' }]} />}</QueryView></div>
}
function RetryNotification({ id }: { id: number }) { const retry = useCommand(`/api/admin/deliveries/${id}/retry`, 'Notification queued for retry'); return <><Button loading={retry.isPending} onClick={() => retry.mutate({})}>Retry</Button><ErrorAlert error={retry.error} /></> }
interface BackupRun { id: number; kind: string; status: string; started_at: string; finished_at: string | null; location: string; details_json: string }
export function BackupsPage() {
  const q = useQuery({ queryKey: ['records', 'backups'], queryFn: () => api.get<{ runs: BackupRun[]; last_backup: BackupRun | null; last_verified_restore: BackupRun | null }>('/api/admin/backups') })
  return <div className="space-y-6"><PageHeader title="Backups and restore checks" description="A restore check verifies an isolated copy of the database and every stored file." /><QueryView query={q}>{d => <><Card title="Latest successful checks"><DescriptionList items={[{ label: 'Last backup', value: d.last_backup ? <DateTime value={d.last_backup.finished_at} /> : 'No successful backup yet' }, { label: 'Last verified restore', value: d.last_verified_restore ? <DateTime value={d.last_verified_restore.finished_at} /> : 'No verified restore yet' }]} /></Card><Table caption="Backup history" rows={d.runs} rowKey={r => r.id} empty={<EmptyState title="No backup runs yet" description="Daily jobs and the backup CLI record their results here." />} columns={[{ key: 'kind', header: 'Check', cell: r => r.kind.replace('_', ' ') }, { key: 'status', header: 'Result', cell: r => <StatusPill status={r.status} /> }, { key: 'at', header: 'Started', cell: r => <DateTime value={r.started_at} /> }, { key: 'where', header: 'Location', cell: r => <span className="break-all">{r.location}</span> }, { key: 'detail', header: 'Details', cell: r => <details><summary>View result</summary><pre className="max-w-md whitespace-pre-wrap break-all text-xs">{r.details_json}</pre></details> }]} /></>}</QueryView></div>
}
interface RetentionRule { id: number; record_class: string; retain_years: number; description: string }
export function RetentionPage() {
  const q = useQuery({ queryKey: ['records', 'retention'], queryFn: () => api.get<RetentionRule[]>('/api/admin/retention-rules') })
  return <div className="space-y-6"><PageHeader title="Retention rules" description="Rules apply when a request closes. Existing retention dates stay unchanged." /><Alert title="Illustrative demonstration policy">These retention periods need Council approval before production use.</Alert><QueryView query={q}>{d => <div className="grid gap-4 lg:grid-cols-2">{d.map(r => <RetentionForm key={`${r.id}-${r.retain_years}-${r.description}`} rule={r} />)}</div>}</QueryView></div>
}
function RetentionForm({ rule }: { rule: RetentionRule }) {
  const [years, setYears] = useState(String(rule.retain_years)), [description, setDescription] = useState(rule.description)
  const save = useCommand('/api/admin/retention-rules', 'Retention rule saved')
  const fields = isApiError(save.error) ? save.error.fields : {}
  return <Card title={rule.record_class === 'default' ? 'General services (default)' : rule.record_class}><form className="space-y-4" onSubmit={e => { e.preventDefault(); save.mutate({ record_class: rule.record_class, retain_years: Number(years), description }) }}><ErrorAlert error={save.error} /><Field label="Years after closure" required error={fields.retain_years}><TextInput type="number" min="0" max="100" step="1" value={years} onChange={e => setYears(e.target.value)} /></Field><Field label="Policy description" required error={fields.description}><Textarea value={description} onChange={e => setDescription(e.target.value)} /></Field><Button type="submit" loading={save.isPending}>Save rule</Button></form></Card>
}
interface Authorities { staff: { id: number; display_name: string }[]; authorities: { id: number; display_name: string; decision_type: string; granted_at: string }[] }
export function AuthorityPage() {
  const { data: me } = useMe()
  const q = useQuery({ queryKey: ['records', 'authority'], queryFn: () => api.get<Authorities>('/api/staff/decision-authorities') })
  const [user, setUser] = useState(''), [type, setType] = useState('development_approval')
  const grant = useCommand('/api/staff/decision-authorities', 'Decision authority granted')
  const fields = isApiError(grant.error) ? grant.error.fields : {}
  return <div className="space-y-6"><PageHeader title="Decision authority" description="A manager grants authority to another active staff member. Systems administration does not confer decision authority." /><QueryView query={q}>{d => <><Card title="Grant authority"><form className="space-y-4" onSubmit={e => { e.preventDefault(); grant.mutate({ user_id: Number(user), decision_type: type }) }}><ErrorAlert error={grant.error} /><Field label="Staff member" required error={fields.user_id}><Select placeholder="Choose a staff member" value={user} onChange={e => setUser(e.target.value)} options={d.staff.filter(u => u.id !== me?.user?.id).map(u => ({ value: String(u.id), label: u.display_name }))} /></Field><Field label="Decision type" required error={fields.decision_type}><Select value={type} onChange={e => setType(e.target.value)} options={['development_approval', 'building_approval', 'modification_approval', 'planning_certificate', 'complaint_response', 'road_response'].map(value => ({ value, label: value.replaceAll('_', ' ') }))} /></Field><Button type="submit" loading={grant.isPending}>Grant authority</Button></form></Card><Table caption="Active authorities" rows={d.authorities} rowKey={r => r.id} empty={<EmptyState title="No decision authorities granted" />} columns={[{ key: 'name', header: 'Staff member', cell: r => r.display_name }, { key: 'type', header: 'Decision', cell: r => r.decision_type.replaceAll('_', ' ') }, { key: 'at', header: 'Granted', cell: r => <DateTime value={r.granted_at} /> }]} /></>}</QueryView></div>
}

function RecoveryControls({user}:{user:StaffUser}) {
  const reactivate=useCommand(`/api/admin/users/${user.id}/reactivate`,'Account reactivated'), password=useCommand<unknown,{one_time_password:string}>(`/api/admin/users/${user.id}/reset-password`,'Password reset'), totp=useCommand(`/api/admin/users/${user.id}/reset-totp`,'TOTP enrolment required')
  return <div className="flex flex-wrap gap-3 mt-4">{!user.is_active && <Button onClick={()=>reactivate.mutate({})}>Reactivate account</Button>}<Button variant="secondary" onClick={()=>password.mutate({})}>Reset password</Button>{user.kind==='staff' && <Button variant="secondary" onClick={()=>totp.mutate({})}>Reset TOTP</Button>}{password.data && <p className="break-all">One-time password: {password.data.one_time_password}</p>}<ErrorAlert error={reactivate.error ?? password.error ?? totp.error}/></div>
}
