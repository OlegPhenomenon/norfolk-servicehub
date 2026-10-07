import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/api/client'
import { useToast } from '@/ui'
export function useCommand<T = unknown, R = unknown>(path: string, message = 'Saved') {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({ mutationFn: (body: T) => api.post<R>(path, body), onSuccess: async () => { toast.success(message); await qc.invalidateQueries({ queryKey: ['records'] }); await qc.invalidateQueries({ queryKey: ['cases'] }) } })
}
