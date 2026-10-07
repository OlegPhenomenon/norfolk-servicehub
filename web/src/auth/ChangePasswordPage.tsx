import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useMutation } from '@tanstack/react-query'
import { api } from '@/api/client'
import { Button, Card, ErrorAlert, Field, PageHeader, TextInput } from '@/ui'
import { useRefreshMe } from './useMe'
export function ChangePasswordPage() {
  const [current,setCurrent]=useState(''),[password,setPassword]=useState(''),refresh=useRefreshMe(),navigate=useNavigate()
  const save=useMutation({mutationFn:()=>api.post('/api/auth/change-password',{current_password:current,new_password:password}),onSuccess:async()=>{const me=await refresh();navigate(me.user?.kind==='staff'?'/staff':'/my')}})
  return <div className="mx-auto max-w-lg space-y-6"><PageHeader title="Change your one-time password"/><Card title="Choose a password"><form className="space-y-4" onSubmit={e=>{e.preventDefault();save.mutate()}}><Field label="Current one-time password" required><TextInput type="password" autoComplete="current-password" value={current} onChange={e=>setCurrent(e.target.value)}/></Field><Field label="New password" required hint="At least 12 characters"><TextInput type="password" autoComplete="new-password" value={password} onChange={e=>setPassword(e.target.value)}/></Field><ErrorAlert error={save.error}/><Button type="submit" loading={save.isPending}>Change password</Button></form></Card></div>
}
