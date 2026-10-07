import { useQuery } from '@tanstack/react-query'
import { api, isApiError } from '@/api/client'
import type { MailboxMessage } from '@/api/types'
import { PageContainer } from '@/layout/PageContainer'
import { Badge, Button, DateTime, EmptyState, ErrorAlert, LoadingState, PageHeader, StatusPill, Table } from '@/ui'

/** `/mock/mail` — DemoMail: outbound email/SMS the demo would have delivered. */
export function DemoMailPage() {
  const query = useQuery({
    queryKey: ['demo', 'mailbox'],
    queryFn: () => api.get<MailboxMessage[]>('/api/demo/mailbox'),
    refetchInterval: 10_000,
  })

  return (
    <PageContainer>
      <PageHeader
        eyebrow={<span className="inline-flex items-center gap-2">DemoMail <Badge tone="warning">Test mode</Badge></span>}
        title="Messages that would have been sent"
        description="In the demonstration, email and SMS go to this simulated gateway instead of real people. Addresses ending in @bounce.example fail on purpose so you can see retries and failures."
        actions={
          <Button variant="secondary" icon="clock" loading={query.isFetching && !query.isPending} onClick={() => void query.refetch()}>
            Refresh
          </Button>
        }
      />
      {query.isPending ? (
        <LoadingState label="Loading messages…" />
      ) : query.isError ? (
        isApiError(query.error, 'not_found') ? (
          <EmptyState icon="lock" title="DemoMail is only available in demo mode" description="A self-hosted installation sends real messages through its configured gateway." />
        ) : (
          <ErrorAlert error={query.error} onRetry={() => void query.refetch()} />
        )
      ) : (
        <Table
          caption="Outbound messages, newest first"
          rows={query.data}
          rowKey={(m) => m.id}
          empty={<EmptyState icon="mail" title="No messages yet" description="Submit a request or act on one as staff — notifications to residents will appear here." />}
          columns={[
            { key: 'when', header: 'When', className: 'w-40 whitespace-nowrap', cell: (m) => <DateTime value={m.sent_at ?? m.created_at} className="text-sm" /> },
            {
              key: 'to',
              header: 'To',
              className: 'w-56',
              cell: (m) => (
                <span className="flex flex-col gap-1">
                  <span className="break-all">{m.to ?? '—'}</span>
                  {m.channel ? <Badge tone="neutral" icon={m.channel === 'sms' ? 'phone' : 'mail'} className="self-start">{m.channel === 'sms' ? 'SMS' : 'Email'}</Badge> : null}
                </span>
              ),
            },
            {
              key: 'message',
              header: 'Message',
              cell: (m) => (
                <details className="group">
                  <summary className="cursor-pointer list-none font-semibold marker:hidden">
                    <span className="underline decoration-line-strong underline-offset-2 group-open:decoration-primary">{m.subject}</span>
                  </summary>
                  <p className="mt-2 max-w-prose text-[0.95rem] whitespace-pre-wrap text-ink/85">{m.body}</p>
                </details>
              ),
            },
            {
              key: 'status',
              header: 'Status',
              className: 'w-48',
              cell: (m) => (
                <span className="flex flex-col items-start gap-1">
                  <StatusPill status={m.status} />
                  {m.error ? <span className="text-sm text-danger">{m.error}</span> : null}
                  {m.external_id ? <span className="font-mono text-xs text-subtle">{m.external_id}</span> : null}
                </span>
              ),
            },
          ]}
        />
      )}
    </PageContainer>
  )
}
