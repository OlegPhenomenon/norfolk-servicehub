# Deploying the demo to norfolk.shelfcompass.com

## How the live demo is actually deployed (Kamal)

The public demo runs on the shelfcompass.com host (arm64, Ubuntu) next to other
apps behind a shared `kamal-proxy`, which terminates TLS (Let's Encrypt) and
routes `norfolk.shelfcompass.com` to the container. Config: `config/deploy.yml`.

```bash
cp .kamal/secrets.example .kamal/secrets          # once; holds only $VAR references
export KAMAL_REGISTRY_PASSWORD=$(gh auth token)   # token needs write:packages (ghcr.io)
export WEBHOOK_SECRET=... MOCK_API_KEY=...        # keep stable between deploys (openssl rand -hex 32)
kamal deploy                                       # local arm64 build → ghcr.io → zero-downtime switch
kamal app logs -f                                  # logs
kamal app exec 'servicehub reset-demo'             # reset demo data now
```

Health check: `/api/health`. Data lives in the `norfolk_data` Docker volume
(`/data`). The demo switches itself to a "demonstration has ended" page at
`DEMO_ENDS_AT`; `kamal app stop` / `kamal remove` takes it down completely.

## Alternative: plain Docker Compose behind nginx or Caddy (e.g. a council's own server)

One Docker container (`servicehub`, Rust binary + React SPA + SQLite on a named
volume) behind the host's existing reverse proxy. The app listens on
`127.0.0.1:8087` — pick a different host port if 8087 is taken (edit the first
number of the `ports:` entry in `docker-compose.prod.yml`, and the proxy config
upstream).

All `docker compose` commands below run from the repository root and work
regardless of CWD because the compose file resolves its own paths relative to
`deploy/`.

## 1. DNS

Point the subdomain at this server's public IP in the domain's DNS zone:

```
norfolk.shelfcompass.com.  A      <server IPv4>
norfolk.shelfcompass.com.  AAAA   <server IPv6>   # if the host has one
```

Wait for it to resolve (`dig +short norfolk.shelfcompass.com`) — Let's Encrypt
needs the name live before it can issue a certificate.

## 2. Get the code

```sh
git clone https://github.com/OlegPhenomenon/norfolk-servicehub.git
cd norfolk-servicehub
```

## 3. Configure

```sh
cp deploy/servicehub.env.example deploy/servicehub.env
openssl rand -hex 32   # → WEBHOOK_SECRET
openssl rand -hex 32   # → MOCK_API_KEY
```

Edit `deploy/servicehub.env`: paste the two secrets and set `DEMO_ENDS_AT` to
one month after launch in RFC 3339 (e.g. `2026-11-07T00:00:00Z`). Everything
else is already set for the demo.

> `deploy/servicehub.env` contains secrets — do not commit it.

## 4. Start the app

```sh
docker compose -f deploy/docker-compose.prod.yml up -d --build
docker compose -f deploy/docker-compose.prod.yml ps        # expect (healthy)
curl -fsS http://127.0.0.1:8087/api/health                 # {"ok":true,"demo_mode":true}
```

The container binds loopback only; nothing is publicly reachable until the
reverse proxy below is in place. First start runs the embedded DB migrations
and seeds fictional demo data only when `DEMO_MODE=true` and the database has no users or cases. Populated databases are preserved.

## 5. Reverse proxy — pick the one this host already runs

### nginx

```sh
sudo cp deploy/nginx/norfolk.shelfcompass.com.conf \
    /etc/nginx/sites-available/norfolk.shelfcompass.com
sudo ln -s ../sites-available/norfolk.shelfcompass.com \
    /etc/nginx/sites-enabled/norfolk.shelfcompass.com
```

The shipped file already contains the HTTPS server block with certbot
certificate paths, so `nginx -t` fails until the cert exists. Bootstrap once:

1. In the installed copy, comment out the whole second `server { ... 443 ... }`
   block, then `sudo nginx -t && sudo systemctl reload nginx`.
2. `sudo certbot --nginx -d norfolk.shelfcompass.com` — issues the
   certificate into `/etc/letsencrypt/live/norfolk.shelfcompass.com/`.
3. Restore the original file (re-copy from the repo), then
   `sudo nginx -t && sudo systemctl reload nginx`.

(Equivalently: `sudo certbot certonly --nginx -d norfolk.shelfcompass.com`
leaves the config untouched — just uncomment the 443 block afterwards.)

### Caddy

Append `deploy/caddy/Caddyfile.snippet` to the host Caddyfile
(usually `/etc/caddy/Caddyfile`), then:

```sh
sudo caddy fmt --overwrite /etc/caddy/Caddyfile
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

Caddy provisions and renews TLS automatically; no certbot step is needed.

## 6. Verify end to end

```sh
curl -fsS https://norfolk.shelfcompass.com/api/health
# → {"ok":true,"demo_mode":true}
```

Open https://norfolk.shelfcompass.com — the demo banner and persona login
should be visible.

## Day-to-day

**Logs** (json-file driver, rotated at 3 × 10 MB — configured in compose):

```sh
docker compose -f deploy/docker-compose.prod.yml logs -f servicehub
docker inspect -f '{{.State.Health.Status}}' servicehub
```

**Update** — see `docs/OPERATIONS.md` "Upgrade with a pre-migration backup"
for the careful version; for the demo:

```sh
docker compose -f deploy/docker-compose.prod.yml exec servicehub \
    servicehub backup /data/backups/pre-upgrade-$(date +%F)
git pull
docker compose -f deploy/docker-compose.prod.yml up -d --build
```

`serve` applies migrations at start; keep the old image tag until the new
container is healthy.

**Backups** — snapshot = SQLite DB + blobs + manifest inside the volume:

```sh
docker compose -f deploy/docker-compose.prod.yml exec servicehub \
    servicehub backup /data/backups/$(date +%F)
docker compose -f deploy/docker-compose.prod.yml exec servicehub \
    servicehub restore-check /data/backups/$(date +%F)
```

Each run needs a fresh directory (`/data/backups/YYYY-MM-DD`); restore-check
verifies integrity, table counts and blob hashes without touching the live DB.
Copy verified snapshots off this host — see `docs/OPERATIONS.md`.

Daily cron (as root; `-T` because cron has no TTY, `%` must be escaped):

```cron
# /etc/cron.d/servicehub-backup — daily at 03:15
15 3 * * * root cd /srv/norfolk-servicehub \
  && docker compose -f deploy/docker-compose.prod.yml exec -T servicehub \
       servicehub backup /data/backups/$(date +\%F-\%H\%M) \
  && docker compose -f deploy/docker-compose.prod.yml exec -T servicehub \
       servicehub restore-check /data/backups/$(date +\%F-\%H\%M)
```

(adjust `/srv/norfolk-servicehub` to the actual clone path)

## Shutting the demo down (≈ one month)

`DEMO_ENDS_AT` makes the site serve "this demonstration has ended — get the
code" automatically after that instant. To reclaim the resources afterwards:

```sh
docker compose -f deploy/docker-compose.prod.yml down      # keeps the volume
docker compose -f deploy/docker-compose.prod.yml down -v   # …also deletes all data
docker volume rm servicehub-data                           # alternative to -v
```

`down -v` is irreversible — take a backup first if anything should survive.
Remove the nginx site / Caddy block and the certbot lineage
(`certbot delete --cert-name norfolk.shelfcompass.com`) when done.

## If the council wants to self-host instead

Same compose file, different `deploy/servicehub.env`:

- `DEMO_MODE=false` — no persona login, no scheduled wipes, no expiry
- `DEMO_RESET_HOURS=0`, `DEMO_ENDS_AT=` empty (both ignored outside demo mode)
- real values for `WEBHOOK_SECRET` / `MOCK_API_KEY`
- `PUBLIC_BASE_URL=https://their-domain`

`serve` runs migrations. Bootstrap the catalogue and first administrator against the same volume:

```sh
docker compose -f deploy/docker-compose.prod.yml exec servicehub servicehub seed-catalogue
docker compose -f deploy/docker-compose.prod.yml exec servicehub \
  servicehub create-admin --email admin@example.org --name "Council administrator"
```

The catalogue includes services, prices, resources, templates, retention rules and holidays, without personas/cases. The first administrator receives sysadmin and manager roles. The CLI prints a one-time password: sign in at `/login`, enrol TOTP and replace the password before using staff functions. Subsequent accounts can be created in the admin UI. Never run `seed-demo` or `reset-demo` against operational data.

Outside demo mode `/mock/**` and DemoPay checkout are disabled. Online Pay is hidden; counter and bank paths remain. Real payment and mail providers require adapters; integration receiver endpoints can be configured in the UI. `docs/OPERATIONS.md` covers account recovery, provider limitations, access control, upgrades and cross-server restore. `restore-check` migrates a fresh destination `DATA_DIR` before verification.
