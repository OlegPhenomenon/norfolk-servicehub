import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/api/client'
import { useToast } from '@/ui'
export function useOperation() {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: ({ path, body }: { path: string; body?: unknown }) =>
      api.post(path, body),
    onSuccess: async () => {
      toast.success('Saved')
      await qc.invalidateQueries()
    },
  })
}
