import { useState } from 'react'
import { useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import { Button, ButtonLink, Card, EmptyState, ErrorAlert, Field, PageHeader, QueryView, Table, TextInput, StatusPill } from '@/ui'
import type { Organisation } from './types'
import { useCommand } from './useCommand'
export function OrganisationPage() {
  const q = useQuery({ queryKey: ['records', 'organisations'], queryFn: () => api.get<Organisation[]>('/api/my/organisations') })
  return <div className="space-y-6"><PageHeader title="My organisations" description="Organisation cases are shared with active members. Revoking membership immediately removes access." /><QueryView query={q}>{orgs => orgs.length === 0 ? <EmptyState title="No organisation memberships" description="Ask your organisation owner to invite your account email." /> : <div className="space-y-5">{orgs.map(o => <OrganisationCard key={o.id} organisation={o} />)}</div>}</QueryView><CreateOrganisation /></div>
}
function OrganisationCard({ organisation: o }: { organisation: Organisation }) {
  const [email, setEmail] = useState('')
  const invite = useCommand(`/api/my/organisations/${o.id}/invites`, 'Invitation sent')
  const fields = isApiError(invite.error) ? invite.error.fields : {}
  return <Card title={o.name}><Table caption="Organisation members" rows={o.members} rowKey={m => m.id} columns={[{ key: 'name', header: 'Member', cell: m => m.display_name ?? m.invite_email }, { key: 'role', header: 'Role', cell: m => m.role }, { key: 'status', header: 'Status', cell: m => <StatusPill status={m.status} /> }, { key: 'action', header: 'Action', cell: m => o.role === 'owner' && m.role !== 'owner' && m.status !== 'revoked' ? <RevokeMember organisationId={o.id} memberId={m.id} /> : '—' }]} />
    {o.role === 'owner' && <form className="mt-5 space-y-4" onSubmit={e => { e.preventDefault(); invite.mutate({ email }, { onSuccess: () => setEmail('') }) }}><ErrorAlert error={invite.error} /><Field label="New member's email address" required error={fields.email}><TextInput type="email" value={email} onChange={e => setEmail(e.target.value)} /></Field><Button type="submit" loading={invite.isPending}>Send invitation</Button></form>}
  </Card>
}
function RevokeMember({ organisationId, memberId }: { organisationId: number; memberId: number }) {
  const revoke = useCommand(`/api/my/organisations/${organisationId}/members/${memberId}/revoke`, 'Access revoked')
  return <><Button variant="danger-outline" loading={revoke.isPending} onClick={() => revoke.mutate({})}>Revoke access</Button><ErrorAlert error={revoke.error} /></>
}
export function InvitePage() {
  const { token } = useParams()
  const accept = useCommand(`/api/my/invites/${encodeURIComponent(token ?? '')}/accept`, 'Invitation accepted')
  return <div className="space-y-6"><PageHeader title="Accept organisation invitation" description="Sign in with the email address that received this invitation." /><Card title="Join the organisation"><ErrorAlert error={accept.error} />{accept.isSuccess ? <ButtonLink to="/my/organisation">View organisation members</ButtonLink> : <Button loading={accept.isPending} onClick={() => accept.mutate({})}>Accept invitation</Button>}</Card></div>
}

function CreateOrganisation() {
  const [name,setName] = useState(''), [abn,setAbn] = useState('')
  const create = useCommand('/api/my/organisations','Organisation created')
  return <Card title="Create organisation"><form className="space-y-4" onSubmit={e => { e.preventDefault(); create.mutate({name,abn:abn || null}, {onSuccess:()=>{setName('');setAbn('')}}) }}><Field label="Organisation name" required><TextInput value={name} onChange={e=>setName(e.target.value)} /></Field><Field label="ABN (if applicable)"><TextInput value={abn} onChange={e=>setAbn(e.target.value)} /></Field><ErrorAlert error={create.error}/><Button type="submit" loading={create.isPending}>Create organisation</Button></form></Card>
}
