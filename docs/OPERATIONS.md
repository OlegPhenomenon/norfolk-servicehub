# Norfolk ServiceHub operations

ServiceHub is one Rust process serving the React application, a SQLite database, and a content-addressed blob directory. All demo people and records are fictional. In demo mode email, payments, Content Manager, and Civica Altitude use in-process mock gateways.

## Install and configure

Use the repository's Dockerfile, or build with current stable Rust and Node 22.13+ (or Node 24). For native installation, Poppler utilities must be on PATH for document processing.

```sh
docker build -t servicehub:local .
docker volume create servicehub-data
cp .env.example .env
```

Set `PUBLIC_BASE_URL` to the external HTTPS address. Set persistent `MOCK_API_KEY` and `WEBHOOK_SECRET` values. `COOKIE_SECURE=true` is required behind HTTPS; for local plain HTTP use `false`. `TRUST_PROXY=true` is appropriate only when a trusted proxy controls the forwarded headers. Restrict access to the data volume and backup directories to the service account and backup operators.

```sh
docker run --rm --env-file .env \
  -e DATA_DIR=/data -e WEB_DIST=/app/web -e SEED_DATA_DIR=/app/seed-data \
  -v servicehub-data:/data servicehub:local servicehub migrate

docker run -d --name servicehub --restart unless-stopped --env-file .env \
  -e DATA_DIR=/data -e WEB_DIST=/app/web -e SEED_DATA_DIR=/app/seed-data \
  -p 127.0.0.1:8080:8080 -v servicehub-data:/data servicehub:local
```

The explicit Docker environment overrides keep the database and files on the mounted volume; `.env.example` otherwise uses native development paths. The reverse proxy should forward to port 8080 and terminate HTTPS. Check `/api/health` after starting the process.

For a native build:

```sh
cd web
npm ci
npm run build
cd ../server
cargo build --release --locked
```

Set `DATA_DIR` to an absolute writable directory, `WEB_DIST` to the built `web/dist`, and `SEED_DATA_DIR` to `server/seed-data`. Export environment variables before invoking `server/target/release/servicehub`; the binary reads the process environment and does not load `.env` itself.

On an explicitly disposable demonstration database, `servicehub seed-demo` creates the personas and module seed data. **Both `seed-demo` and `reset-demo` wipe existing records and files.** Do not use either command on operational data. Set `DEMO_MODE=false` for operational installations. `DEMO_RESET_HOURS=0` disables periodic reset in demo mode; `DEMO_ENDS_AT` only applies to demos.

## First administrator and catalogue

On a fresh self-hosted database, use the same `DATA_DIR` and configuration as `serve`:

```sh
servicehub seed-catalogue
servicehub create-admin --email admin@example.org --name "Council administrator"
```

Both commands migrate first. `seed-catalogue` is repeatable and creates services, prices, resources, templates, retention rules and holidays, without personas or cases. `create-admin` prints a random one-time password. Deliver it privately to the named administrator; do not retain it in deployment logs. The first administrator also receives the manager role so they can grant manager and complaints-officer roles to other staff. Later administrators receive sysadmin only. Interactive grants cannot be made to oneself; manager and complaints-officer grants require a manager.

Sign in at `/login`, enrol an authenticator, confirm TOTP and change the one-time password. Case and staff APIs remain blocked until these steps are complete. Administrators can deactivate/reactivate accounts and reset another user's password or staff TOTP at `/admin/users`; resets revoke sessions and force password replacement or authenticator enrolment respectively.

A fresh `serve` with `DEMO_MODE=true` automatically seeds fictional history once when there are no users or cases. It does not overwrite populated installations. Explicit `seed-demo` and `reset-demo` are destructive reset commands.

## Demo mode vs self-hosted

`DEMO_MODE` chooses between two operating modes of the same binary:

- **Demo (`DEMO_MODE=true`).** The `/demo` persona-picker page and its sign-in endpoints are enabled, and staff personas get a one-click TOTP via the on-screen demo authenticator. `DEMO_RESET_HOURS` (default `6`, `0` disables) schedules a wipe-and-reseed of all data — everything visitors create is deleted. A banner shows the next reset. With `DEMO_ENDS_AT` set (RFC 3339 timestamp), the site serves a static "demo has ended" page and API requests return `410` once the time passes; `REPO_URL` is linked from that page so visitors can run their own copy.
- **Self-hosted (`DEMO_MODE=false`, the default).** `/demo`, the persona endpoints and the demo authenticator return `404`. There is no scheduled reset and `DEMO_ENDS_AT` has no effect — the installation has no expiry or phone-home. Bootstrap the first administrator with `create-admin` as above, then create further accounts at `/admin/users`; staff enrol their own authenticator at first login. Catalogue seeding uses `seed-catalogue`. Fictional seed data is optional and `seed-demo`/`reset-demo` must never be run against operational data — both wipe the database and blobs.

Migrations, backups and restore tooling are shared. All `/mock/**` routes, including DemoPay checkout, are absent outside demo mode. Online checkout returns "Online payment is not configured" and the Pay button is hidden; counter receipts and bank statement matching remain available. Real online payment and email/SMS delivery require provider adapters; queued notifications do not become delivered through a mock in self-hosted mode. AI is enabled by default but its suggestion endpoint is a demo mock; set `AI_ENABLED=false` to hide it. Integration endpoints are configurable at `/admin/integrations`; the supplied receivers illustrate the exchange contract, without claiming verified Council connectivity.

## Upgrade with a pre-migration backup

1. Build and test the new image without replacing the running service.
2. Take a backup using the currently installed binary, and run its restore check.
3. Stop the running service, preserve its environment configuration, then start the new image with the same data volume. `serve` applies embedded migrations before accepting requests.
4. Check `/api/health`, a resident case, a staff case, and the worker's delivery logs.

Never edit an applied migration. Keep the old image and verified backup together. If an upgrade must be rolled back after a schema change, restore the pre-upgrade database and files using the old image; running the old binary on a newer schema is not a rollback.

## Back up and verify

Choose a new directory for each snapshot; existing snapshots are never overwritten.

```sh
DATA_DIR=/srv/servicehub/data servicehub backup /srv/servicehub/backups/2026-10-07
DATA_DIR=/srv/servicehub/data servicehub restore-check /srv/servicehub/backups/2026-10-07
```

A backup contains `servicehub.db`, `blobs/<prefix>/<sha256>`, and `manifest.json`. The manifest records the application version, timestamp, per-table counts, and every copied blob's SHA-256. The source writer lock remains held during the SQLite snapshot and file copy. `VACUUM INTO` runs on a dedicated read connection because SQLite prohibits it inside the write transaction. Large backups temporarily delay case writes; schedule them outside busy hours.

The restore check copies the backup into a unique scratch directory under `DATA_DIR`, opens that isolated database, performs `integrity_check` and `foreign_key_check`, compares all table counts and blob inventories to the manifest, verifies every file hash, and reads a document for every case that has a live document. It never replaces the live database. Success/failure details appear in `backup_runs` and `/admin/backups`. CLI failure has a non-zero exit status.

The `records.backup` job performs a backup and restore check, then queues the next day's job. Demo seeding queues the first run; for a fresh, unseeded installation the platform scheduler enqueues it on its next tick (`jobs::schedule_tick` calls `records::schedule_daily`, deduplicated per day). No external timer is required. Results are recorded in `backup_runs` and visible at `/admin/backups`; alert operators when a run records a failure. You may additionally run the two CLI commands above from a system timer — a second snapshot never overwrites an existing one.

Copy verified snapshots off the application server. Preserve access controls and encryption while transporting/storing backups: the database contains account data and confidential case content. Restore checks verify consistency and integrity, not an external signature against malicious replacement of the entire manifest.

## Restore to another server

1. Stop the destination service. Install the same application version as the backup and configure its environment. Keep all database connections closed during replacement.
2. Copy the complete snapshot to the destination. Run `restore-check` with `DATA_DIR` pointing at a writable scratch/application directory (it migrates this directory first, so a fresh directory works); inspect the successful result before proceeding.
3. Create an empty destination `DATA_DIR` owned by the service account. Copy `servicehub.db` and the entire `blobs/` directory into it. Do not copy stale `-wal` or `-shm` files from the former database.
4. Restore configuration secrets separately. Configure the new public and internal base URLs and reverse proxy. Application secrets are not part of `manifest.json`.
5. Start the service using the same application version, then verify `/api/health`, case counts, an authorised document download, confidential access restrictions, and the last verified restore record. Upgrade only after this restore is working.

Compare case counts with the snapshot's manifest. For a more detailed check, compare the manager dashboard and drill-down lists over the same period and confirm a record with an external reference. Outbound jobs retained in a snapshot may run again; the integrations outbox preserves stable operation IDs and the mock receiver deduplicates them.

## Deliveries and confidential access

`/admin/integrations` shows accepted, failed and stopped records deliveries. To demonstrate recovery, enable a mock outage, create an outbound operation, inspect its failure, disable the outage, and retry. The response-loss switch accepts an operation but delays its first response beyond the client's five-second timeout; retries return the original external reference.

A systems administrator sees diagnostics, not case content, unless another role grants case access. Complaint subject exclusions override every role. Role changes and membership revocations take effect on the next request; confidential notifications use only the case reference.

Notification team copies use the addresses saved at `/admin/settings` for Customer Care, Finance and Works Depot. Confidential case content is not copied to shared team addresses.

Notification retry creates a new outbound attempt through the shared notification API and keeps the original failed row for history. Retention rules cover case modules and document categories (`document:letter`, `document:receipt`, etc.) and are explicitly illustrative policies. Closure records each document's date; whole-case disposal waits for the longest retained record. Changing a rule affects future closure dates; it does not shorten dates already recorded. A legal hold prevents disposal. Disposal integration must preserve metadata, decision text and exact evidence version IDs while removing files only when no other live reference exists.
