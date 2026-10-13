# Install Procera with Docker Compose

This guide runs a complete Procera installation on one machine with Docker Compose: the database, the
server and the web app. It takes about 20 minutes, most of it the first build.

> **Status: pre-release.** There are no published images yet, so Compose builds Procera from this
> repository. Files are stored on local disk (a Docker volume) by default; set `PROCERA_STORAGE=s3`
> to keep them in an S3-compatible bucket instead (see the configuration reference). External key
> management (KMS) is designed but not built yet.

Next steps after this guide: [First-time setup](first-time-setup.md) and the
[configuration reference](configuration.md).

## What you get

`deploy/compose.yml` starts these containers:

| Service | Image | What it does |
|---|---|---|
| `postgres` | `postgres:16` | The database. Data in the `pgdata` volume. On the first start it creates Procera's database roles (`deploy/sql/00-roles.sql`) and sets their passwords. |
| `migrate` | built from `Dockerfile` | Runs `procera migrate` as the `procera_migrator` role, then exits. Runs on every `up`, before the server starts. |
| `procera` | built from `Dockerfile` | Runs `procera serve --all`: the API, the background worker (email, events) and the scheduler in one process. Uploaded files go to the `files` volume. Not published to the host. |
| `web` | built from `deploy/web/Dockerfile` | Caddy. Serves the web app (`web/dist`) and forwards `/api`, `/sysapi`, `/healthz`, `/readyz` and `/version` to `procera`, so the browser talks to one address. The only service with published ports. |

The `procera` server serves the API only; it does not serve the web app's files. That is why the `web`
container exists. If you already run a reverse proxy, see [Using your own reverse proxy](#using-your-own-reverse-proxy).

Optional services, each behind a Compose profile:

| Profile | Service | Turn on with | Then set in `.env` |
|---|---|---|---|
| `cache` | Valkey 8 | `--profile cache` | `PROCERA_CACHE_URL=redis://valkey:6379` |
| `scan` | ClamAV | `--profile scan` | `PROCERA_CLAMD_ADDR=clamav:3310` |
| `mail` | Mailpit (a test inbox at http://127.0.0.1:8025) | `--profile mail` | `PROCERA_SMTP_URL=smtp://mailpit:1025` |

Without the cache, Procera uses an in-process cache, which is fine for one server. Without ClamAV, uploads
are checked only for the EICAR test file, so turn on `scan` for real use. Mailpit catches email for
testing; it never delivers to real inboxes.

## Prerequisites

- Docker Engine with the Compose v2 plugin (`docker compose version` works).
- Memory: about 4 GB free for the first build (it compiles the Rust server); about 1 GB to run.
  ClamAV needs roughly 2 to 3 GB more.
- Disk: about 10 GB for build caches and images, plus your data.
- A clone of this repository. Run every command below from its root.
- For production: a domain name pointing at the machine, and ports 80 and 443 open (see [HTTPS](#https)).

## 1. Create the env file

All settings and secrets live in one file, `deploy/.env`. Compose reads it automatically because it sits
next to `deploy/compose.yml`. It is ignored by git; keep it that way.

```bash
cp deploy/.env.example deploy/.env
```

Fill in the three database passwords. Hex passwords are safest because they go into connection URLs:

```bash
openssl rand -hex 24   # run it three times, once per password
```

- `POSTGRES_PASSWORD` is the database superuser. Only the init scripts and your backups use it.
- `PROCERA_MIGRATOR_DB_PASSWORD` is for `procera_migrator`, which owns the schema and runs migrations.
- `PROCERA_APP_DB_PASSWORD` is for `procera_app`, the role the server uses. It can read and write rows
  but never change the schema, and row-level security always applies to it.

The role passwords are set **once**, when the `pgdata` volume is first created. Changing them in `.env`
later does not change them in the database (see [Troubleshooting](#troubleshooting)).

## 2. Generate the master key

The master key protects everything Procera encrypts: each workspace's keys, stored files, two-step
verification secrets and the saved email password. Generate it once:

```bash
docker compose -f deploy/compose.yml run --rm --no-deps -T procera keygen
```

The first time, this builds the server image, which takes a while. The command prints a base64 key on
its own line (and a log line about it). Put the key in `deploy/.env`:

```dotenv
PROCERA_MASTER_KEY=…the printed key…
```

`openssl rand -base64 32` produces an equivalent key if you prefer.

> **Back up the master key now**, separately from your database backups, for example in a password
> manager or secret store. **If you lose it, the encrypted data in the database and the stored files
> cannot be read again.** Nobody can recover it for you. Don't change it on a running installation either:
> data encrypted with the old key would become unreadable.

The server refuses to start with a database but without a master key.

## 3. Set the address

`PROCERA_PUBLIC_URL` is the address people type into their browser. Procera uses it in email links and
for passkeys.

- Trying it on your own computer: keep `PROCERA_PUBLIC_URL=http://localhost:8080` and
  `PROCERA_HTTP_PORT=8080`.
- A server with a domain: see [HTTPS](#https).

## 4. Start Procera

```bash
docker compose -f deploy/compose.yml up -d --build
```

With optional services, add profiles, for example
`docker compose -f deploy/compose.yml --profile scan --profile mail up -d --build`, or set
`COMPOSE_PROFILES=scan,mail` in `deploy/.env`. Remember to set the matching variables (table above).

Compose starts `postgres`, waits until it's healthy, runs `migrate`, starts `procera` once migrations
succeed, and starts `web` once `procera` answers its health check. Check the state:

```bash
docker compose -f deploy/compose.yml ps
curl -s http://localhost:8080/healthz    # {"status":"ok"}
curl -s http://localhost:8080/readyz     # {"status":"ready","database":"ok",…}
```

`migrate` shows as exited (0); that is expected.

## 5. Find the one-time setup token

There are no default accounts. The first time Procera starts, it prints a one-time **setup token** to its
log. The token is valid for 24 hours and works once.

```bash
docker compose -f deploy/compose.yml logs procera | grep bootstrap_token
```

The log is JSON; the token is the value of `bootstrap_token`, in a line that says
`platform not bootstrapped`. If it has expired, restart the server to print a new one:
`docker compose -f deploy/compose.yml restart procera`. Tokens are printed only until setup is finished.

## 6. Open /setup

Open `http://localhost:8080/setup` (or `https://your-domain/setup`) and paste the token. Then follow
[First-time setup](first-time-setup.md): you create the system owner, turn on two-step verification, set
up email and create the first workspace.

## HTTPS

Run Procera over HTTPS for anything beyond a local trial. Sign-in cookies are `Secure` with the `__Host-`
prefix, so browsers only send them over HTTPS (they make an exception for `http://localhost`).

The bundled Caddy can get and renew a certificate for you. Point a DNS record at the machine, open ports
80 and 443, and set in `deploy/.env`:

```dotenv
PROCERA_SITE_ADDRESS=procera.example.com
PROCERA_HTTP_PORT=80
PROCERA_HTTPS_PORT=443
PROCERA_PUBLIC_URL=https://procera.example.com
```

Then `docker compose -f deploy/compose.yml up -d`. Certificates are kept in the `caddy_data` volume.

### Using your own reverse proxy

If a reverse proxy already terminates TLS on the machine, keep `PROCERA_SITE_ADDRESS=:80`, publish the
`web` container on a local port (for example `PROCERA_HTTP_PORT=127.0.0.1:8080`) and proxy your domain
to it. The proxy must replace any `X-Forwarded-For` header from the client with the real client address:
Procera takes the first address in that header for sign-in rate limits and the audit log. The
[configuration guide](configuration.md#reverse-proxy-and-tls) has Caddy and nginx examples, including
running without the `web` container.

## Backups

Back up three things, and test restoring them:

1. **The database.**
2. **The `files` volume** (uploaded files, encrypted).
3. **The master key** (`PROCERA_MASTER_KEY`). Without it, 1 and 2 are unreadable. Keep it apart from the
   other backups.

For a consistent pair, stop the server briefly, back up, and start it again:

```bash
docker compose -f deploy/compose.yml stop procera

# 1. Database (custom format, compressed)
docker compose -f deploy/compose.yml exec -T postgres \
  pg_dump -U postgres -Fc procera > procera-db-$(date +%F).dump

# 2. Files volume (the volume is named <project>_files; the project is "procera")
docker run --rm -v procera_files:/data:ro -v "$PWD":/backup alpine \
  tar czf /backup/procera-files-$(date +%F).tgz -C /data .

docker compose -f deploy/compose.yml start procera
```

Keep `deploy/.env` safe too: it holds the database passwords.

### Restoring

Restore into a fresh stack with the **same `deploy/.env`** (above all the same master key):

```bash
docker compose -f deploy/compose.yml down              # keep volumes unless you are replacing them
docker volume rm procera_pgdata procera_files          # only when replacing the data
docker compose -f deploy/compose.yml up -d postgres     # creates the roles and their passwords
docker compose -f deploy/compose.yml exec -T postgres \
  pg_restore -U postgres -d procera --clean --if-exists < procera-db-YYYY-MM-DD.dump
docker run --rm -v procera_files:/data -v "$PWD":/backup alpine \
  sh -c 'tar xzf /backup/procera-files-YYYY-MM-DD.tgz -C /data && chown -R 65532:65532 /data'
docker compose -f deploy/compose.yml up -d
```

`pg_restore` may report that the `discovery` schema or the extensions already exist; those notices are
harmless. The files volume must stay owned by user 65532, the server's non-root user.

## Upgrades

Procera has no packaged releases yet, so upgrading means updating your checkout and rebuilding:

```bash
# back up first (see above)
git pull
docker compose -f deploy/compose.yml up -d --build
```

`migrate` applies new migrations before the new server starts. Migrations only move forward: to go
back to an older version, restore the backup you took before upgrading. Read the release notes or commit
log for changes to environment variables.

## Day-to-day commands

```bash
docker compose -f deploy/compose.yml ps                  # what is running
docker compose -f deploy/compose.yml logs -f procera     # server logs (JSON lines)
docker compose -f deploy/compose.yml restart procera     # restart the server
docker compose -f deploy/compose.yml down                # stop everything, keep data
docker compose -f deploy/compose.yml down -v             # stop and DELETE all data volumes
```

Change the log detail with `PROCERA_LOG` (for example `PROCERA_LOG=debug`), then
`docker compose -f deploy/compose.yml up -d`.

## Troubleshooting

**`required variable POSTGRES_PASSWORD is missing a value`** (or one of the other passwords).
Fill it in `deploy/.env`. Compose refuses to start without the three database passwords.

**`procera` exits with `PROCERA_MASTER_KEY is required when a database is configured`.**
Set `PROCERA_MASTER_KEY` (step 2). `PROCERA_MASTER_KEY must be 32 bytes (base64)` means the value is
truncated or isn't a key from `procera keygen`.

**`migrate` fails with `password authentication failed for user "procera_migrator"`.**
The role passwords were set when the `pgdata` volume was created, and `.env` has changed since. Either
put the original passwords back, or set the new ones in the database:

```bash
docker compose -f deploy/compose.yml exec postgres psql -U postgres -d procera \
  -c "ALTER ROLE procera_migrator PASSWORD 'new-migrator-password'" \
  -c "ALTER ROLE procera_app PASSWORD 'new-app-password'"
```

On a trial install with no data you want to keep, `docker compose -f deploy/compose.yml down -v` starts
from scratch.

**There is no `bootstrap_token` line in the log.** Either setup is already finished (opening `/setup`
then says "Procera is already set up"; sign in instead), or the log has rotated. Restart `procera` to
print a fresh token while setup is unfinished.

**"This setup token doesn't work".** Each token works once and expires after 24 hours. A rejected
password or an email that already exists also uses the token up. Restart `procera` for a new one.

**Signing in works, but you're signed out again straight away.** The browser isn't keeping the `Secure`
cookie because the address is plain HTTP and not `localhost`. Use HTTPS. For a short trial on a private
network only, set `PROCERA_INSECURE_COOKIES=true`.

**Emails aren't arriving.** The log says `email delivery isn't set up: emails are queued but not sent`
until you set up email in the system console (Email & platform) or set `PROCERA_SMTP_URL`. Queued emails
go out once delivery works. With Mailpit, use `PROCERA_SMTP_URL=smtp://mailpit:1025`: the console's form
allows unencrypted delivery only to `localhost`, and Mailpit's address in Compose is `mailpit`.

**Links in emails point to the wrong address.** Set `PROCERA_PUBLIC_URL` to the address people use.
System › Email & platform shows the value the server is using and where it came from.

**Uploads say they aren't scanned for real malware.** Turn on the `scan` profile and set
`PROCERA_CLAMD_ADDR=clamav:3310`. ClamAV downloads its signatures on first start, which can take several
minutes; `docker compose -f deploy/compose.yml ps` shows it as healthy when ready.

**The port is already in use.** Change `PROCERA_HTTP_PORT` (and `PROCERA_PUBLIC_URL` to match).

**The build runs out of memory.** Give Docker more memory (Docker Desktop: Settings › Resources), or
build on a bigger machine and copy the images.
