# `ui/` — Norfolk ServiceHub design system

Small, accessible components built on Tailwind CSS 4. **Use these instead of raw HTML controls** so every
screen looks and behaves the same. Import everything from `@/ui`:

```tsx
import { Button, Card, Field, TextInput, StatusPill, Money, DateTime, useToast } from '@/ui'
```

## Rules of thumb

- Colours: use the semantic tokens from `src/index.css` — `bg-canvas`, `bg-surface`, `bg-sunken`, `border-line`,
  `text-ink`, `text-muted`, `text-subtle`, `bg-primary` / `text-primary` (ocean blue, with `-50…-900`),
  `bg-pine` / `text-pine` (Norfolk-pine green accent), `text-danger`, `bg-warning-50`, … Never hard-code hex values.
- Fonts: body is Inter (`font-sans`); page titles use Source Serif (`font-serif`) — `PageHeader` does this for you.
- Every page has exactly one `<h1>`: render a `PageHeader` at the top. Cards use `h2` (`headingLevel` to change).
- Touch targets ≥ 44 px: `Button` `md`/`lg` sizes, nav links and inputs already comply; `size="sm"` only in dense tables.
- Focus rings are global (yellow outline on `:focus-visible`). Don't remove outlines.
- Money is integer cents → `<Money cents={…} />`. Times are UTC strings → `<DateTime value={…} />` (shown in Norfolk time).
- Statuses → `<StatusPill status={row.status} />`; never hand-colour a status.
- Data fetching: `useQuery` + `api` from `@/api/client`; render with `<QueryView>`; show mutation errors with `<ErrorAlert>`;
  show field errors from `ApiError.fields` in `<Field error>`; confirm success with `useToast().success(...)`.
- Page layout: wrap public pages in `<PageContainer>` (`@/layout/PageContainer`); `/my`, `/staff`, `/admin` layouts
  already pad their content.

## Catalogue

### Actions

| Component | Usage |
|---|---|
| `Button` | `<Button onClick={save}>Save</Button>` · `variant`: `primary` (default) · `secondary` · `ghost` · `danger` · `accent` (green) · `size`: `sm` · `md` · `lg` · `icon` / `iconRight` (IconName) · `loading` · `fullWidth`. Defaults to `type="button"`; pass `type="submit"` in forms. |
| `ButtonLink` | Router link styled as a button: `<ButtonLink to="/services" icon="search">Find a service</ButtonLink>` |
| `buttonClasses(variant, size, fullWidth)` | Class string for other elements, e.g. `<a href={pdfUrl} className={buttonClasses('secondary')}>Download</a>` |
| `Icon` | `<Icon name="calendar" />` (decorative) · `<Icon name="bell" title="Notifications" />` (meaningful). Names: see `IconName` in `Icon.tsx` (home, inbox, folder, file, calendar, truck, wrench, map, pin, coins, receipt, chart, shield, lock, settings, users, user, bell, search, plus, check, x, chevron*, arrowRight, menu, external, alert, info, checkCircle, xCircle, clock, mail, phone, logout, building, book, archive, key, message, clipboard, sparkles, upload, download, pine). |

### Forms

Wrap each control in a `Field`; it wires `id`, `aria-describedby`, `aria-invalid`, `aria-required` automatically.
Fields are marked "(optional)" unless `required` — this is deliberate (GOV.UK style).

```tsx
const save = useMutation({ mutationFn: () => api.post('/api/things', form) })
const errors = isApiError(save.error) ? save.error.fields : {}

<form onSubmit={(e) => { e.preventDefault(); save.mutate() }} noValidate className="flex flex-col gap-5">
  {save.error && !isApiError(save.error, 'validation') ? <ErrorAlert error={save.error} /> : null}
  <Field label="Event name" hint="Shown on your booking confirmation." error={errors.event_name} required>
    <TextInput value={name} onChange={(e) => setName(e.target.value)} maxLength={120} />
  </Field>
  <Field label="Details" error={errors.details}>
    <Textarea rows={5} value={details} onChange={(e) => setDetails(e.target.value)} />
  </Field>
  <Field label="Room" error={errors.room} required>
    <Select placeholder="Choose a room" options={[{ value: 'main', label: 'Main hall' }]} value={room} onChange={(e) => setRoom(e.target.value)} />
  </Field>
  <Field label="Site plan (PDF)" error={errors.file} required>
    <FileInput accept="application/pdf" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
  </Field>
  <RadioGroup legend="Will alcohol be served?" name="alcohol" required value={alcohol} onChange={setAlcohol}
    options={[{ value: 'no', label: 'No' }, { value: 'yes', label: 'Yes' }]} error={errors.alcohol} inline />
  <Checkbox label="I confirm the information is correct" checked={ok} onChange={(e) => setOk(e.target.checked)} error={errors.confirm} />
  <Button type="submit" loading={save.isPending}>Submit</Button>
</form>
```

| Component | Notes |
|---|---|
| `Field` | `label`, `hint`, `error`, `required`, `hideLabel`. One control inside. |
| `TextInput`, `Textarea`, `Select`, `FileInput` | Native props pass through. `Select` takes `options` + optional `placeholder`. |
| `Checkbox` | Stand-alone (has its own label/hint/error) — not inside `Field`. |
| `RadioGroup` | Fieldset + legend; prefer over `Select` for 2–5 options. |
| `controlClasses` | Class string to style a custom input like the others. |

### Layout & content

| Component | Usage |
|---|---|
| `PageHeader` | `<PageHeader eyebrow="NSH-2026-000123" title="Hire of Rawson Hall" description="…" meta={<StatusPill status="in_progress" />} breadcrumbs={[{ label: 'My requests', to: '/my' }, { label: 'Hire' }]} actions={<Button>Pay now</Button>} />` |
| `Card` | `<Card title="Applicant" description="…" actions={…} footer={…} padded={false}>…</Card>` — `padded={false}` for edge-to-edge tables/lists. |
| `DescriptionList` | `<DescriptionList columns={2} items={[{ label: 'Number', value: c.number }, …]} />` (empty values show "—"). |
| `Tabs` | `<Tabs label="Case sections" tabs={[{ key: 'overview', label: 'Overview', count: 3, content: <Overview /> }]} value={tab} onChange={setTab} />` — arrow-key accessible; controlled or uncontrolled. |
| `Table` | `<Table caption="Open cases" rows={rows} rowKey={(r) => r.id} columns={[{ key: 'n', header: 'Number', cell: (r) => <Link className="link" to={…}>{r.number}</Link> }, { key: 'amt', header: 'Amount', align: 'right', cell: (r) => <Money cents={r.total_cents} /> }]} empty={<EmptyState title="Nothing here" />} />` — scrolls horizontally on phones; `dense`; `rowClassName`; hide columns on mobile via `className: 'hidden md:table-cell'`. |
| `Timeline` | `<Timeline items={events.map((e) => ({ id: e.id, at: e.created_at, title: e.summary, actor: e.actor_name, body: e.detail, tone: 'success', icon: 'check' }))} />` |
| `Steps` | Workflow progress: `<Steps label="Progress" current={c.current_step} finished={c.status === 'completed'} steps={def.workflow.steps.map((s) => ({ key: s.key, label: s.applicant_label }))} />` · `orientation="horizontal"` · per-step `state`: `complete`/`current`/`upcoming`/`skipped`/`failed`. |
| `EmptyState` | `<EmptyState icon="inbox" title="No requests yet" description="…" action={<ButtonLink to="/services">Find a service</ButtonLink>} />` |

### Status & data display

| Component | Usage |
|---|---|
| `Badge` | `<Badge tone="accent" icon="lock">Confidential</Badge>` · tones: `neutral` `info` `primary` `success` `warning` `danger` `accent`. |
| `StatusPill` | `<StatusPill status="waiting_on_applicant" />` — labels/tones for every status in the schema live in `status.ts` (`STATUS_STYLES`); unknown values are humanised. Override with `label` / `tone`. |
| `Money` | `<Money cents={18000} />` → `$180.00`; `signed` colours negatives green. `formatMoney(cents)` for strings. |
| `DateTime` | `<DateTime value={ts} format="date" />` — `date` (7 Oct 2026) · `datetime` (default) · `time` · `long` · `short` · `relative` ("2 hours ago", absolute in tooltip). Accepts `YYYY-MM-DD` local dates without shifting. Helpers: `formatDateTime`, `formatRelative`, `norfolkToday()`, `NORFOLK_TZ`. |
| `humanize('in_progress')` | → "In progress". |

### Feedback

| Component | Usage |
|---|---|
| `Alert` | `<Alert tone="warning" title="Payment required" actions={<Button>Pay</Button>}>…</Alert>` · tones `info` `success` `warning` `danger`; `onDismiss`. |
| `ErrorAlert` | `<ErrorAlert error={mutation.error} />` · `<ErrorAlert error={q.error} onRetry={() => q.refetch()} />` — shows `ApiError.message` and field messages. |
| `QueryView` | `<QueryView query={q} loading="Loading bookings…">{(data) => …}</QueryView>` — spinner / error with retry / data. |
| `Spinner`, `LoadingState` | `<Spinner />` inline · `<LoadingState label="Loading…" />` block. |
| `useToast()` | `toast.success('Saved')` · `toast.info(msg)` · `toast.error(errorOrString)` — short confirmations after actions. Provider is mounted in `App.tsx`. |
| `Dialog` | `<Dialog open={open} onClose={() => setOpen(false)} title="Request information" description="…" footer={<><Button variant="secondary" onClick={…}>Cancel</Button><Button onClick={…}>Send</Button></>}>…</Dialog>` — native `<dialog>`: focus trap, Escape, backdrop click. `size`: `sm` `md` `lg`. |
| `ConfirmDialog` | `<ConfirmDialog open={o} onClose={…} title="Withdraw this request?" tone="danger" confirmLabel="Withdraw" onConfirm={async () => { await api.post(…) }}>You can't undo this.</ConfirmDialog>` — async-aware, shows errors inline. |

### Utilities

| Name | Usage |
|---|---|
| `cn(...classes)` | `cn('px-4', active && 'bg-primary-50')` |

## Outside `ui/` but used everywhere

- `@/api/client`: `api.get/post/put/patch/delete/upload`, `ApiError`, `isApiError(e, code?)`, `newIdempotencyKey()`.
- `@/api/types`: platform + service-definition types (`Me`, `Role`, `ROLE_LABELS`, `CaseSummary`, `FieldDef`, `AnswerValue`, `ServiceDefinition`, …).
- `@/auth/useMe`: `useMe()`, `hasRole(me, role)`, `hasAnyRole(me, roles)`, `useRefreshMe()`.
- `@/layout/PageContainer`: standard width/padding for public pages.
- `@/featureTypes`: `NavItem`, `CasePanel`, `FieldComponent`, `FieldComponentProps`.
- `@/registry`: `panelsFor(caseSummary, 'staff' | 'applicant')`, `fieldTypes`, `visibleNav(items, roles)`.
- Query keys: start with your module name — `['operations', 'bookings', id]` — so invalidation stays local.
