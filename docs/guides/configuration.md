# Configuration reference

Procera is configured in three places:

1. **The server**: command-line options and `PROCERA_*` environment variables, read at start-up. These
   are for whoever runs the installation.
2. **The system console** (`/system`): email delivery, quotas and platform floors, changed in the app
   by system administrators and recorded in the system audit log.
3. **Workspace Settings** (**Admin › Settings**): everything about how one workspace works, changed by
   its owner and admins and recorded in its audit log.

See also: [Install with Docker Compose](install-docker-compose.md) and [First-time setup](first-time-setup.md).

## 1. Server options and environment variables

The `procera` binary has four commands:

| Command | What it does |
|---|---|
| `procera serve` | Runs the server. Add `--all` to run the API, the background worker (event dispatch, email sending) and the scheduler in one process. Without `--all`, only the API runs: nothing sends email or publishes scheduled versions. Use `--all` today. |
| `procera migrate` | Applies database migrations. Run it as the `procera_migrator` role, never the app role. |
| `procera keygen` | Prints a new random master key (base64) for `PROCERA_MASTER_KEY` on standard output. |
| `procera healthcheck` | Exits 0 if the server on `PROCERA_BIND` answers `GET /healthz` with 200, else 1 (loopback is used when bound to `0.0.0.0`). For container health checks. |

Every option can be given as a flag or as its environment variable; a flag wins. An empty variable
counts as not set. Logs are JSON lines on standard error; standard output is reserved for command output.

### `procera serve`

| Variable | Flag | Default | Required | Purpose |
|---|---|---|---|---|
| `PROCERA_BIND` | `--bind` | `127.0.0.1:8080` (the container image sets `0.0.0.0:8080`) | No | Address and port the HTTP API listens on. |
| (none) | `--all` | off | Yes, in practice | Run the worker and scheduler in the same process (see above). |
| `PROCERA_DATABASE_URL` | `--database-url` | none | Yes | PostgreSQL URL for the runtime role `procera_app`, e.g. `postgres://procera_app:PASSWORD@db:5432/procera`. Without it the server starts in a smoke-test mode with no accounts or data. The server refuses to start if any workspace table lacks forced row-level security. |
| `PROCERA_MASTER_KEY` | `--master-key` | none | Yes, with a database | 32 random bytes, base64 (`procera keygen` or `openssl rand -base64 32`). Root key for secrets at rest (workspace keys, stored files, two-step secrets, the saved SMTP password) and for token signing. |
| `PROCERA_PUBLIC_URL` | `--public-url` | `http://127.0.0.1:8080` | Yes, in practice | The address people use in the browser. Used in email links and as the default passkey relying party. System › Email & platform shows the value in use and where it came from. |
| `PROCERA_WEBAUTHN_RP_ID` | `--webauthn-rp-id` | the host of `PROCERA_PUBLIC_URL` (a loopback IP maps to `localhost`) | No | Passkey relying-party id, a registrable domain such as `example.com`. Changing it later invalidates registered passkeys. |
| `PROCERA_SMTP_URL` | `--smtp-url` | none | No | SMTP relay, e.g. `smtp://user:pass@host:587?tls=required` (STARTTLS) or `smtps://user:pass@host:465` (implicit TLS). Wins over the email settings saved in the system console. With neither, emails stay queued. |
| `PROCERA_MAIL_FROM` | `--mail-from` | `Procera <no-reply@localhost>` | With `PROCERA_SMTP_URL` | Sender used with `PROCERA_SMTP_URL`. Console settings carry their own sender. |
| `PROCERA_CACHE_URL` | `--cache-url` | none (in-process cache) | No | Valkey/Redis URL for caches and rate limits, e.g. `redis://valkey:6379`. The cache is advisory: if it's unreachable at start-up the server logs a warning and uses the in-process cache. Use a shared cache once you run more than one server process. |
| `PROCERA_INSECURE_COOKIES` | `--insecure-cookies` | `false` | No | Use non-`Secure` cookies without the `__Host-` prefix. **Only** for local HTTP development and tests. |
| `PROCERA_LOGIN_RATE_PER_MIN` | `--login-rate-per-min` | `30` | No | Sign-in attempts allowed per client IP per minute. Raise only for test environments where everyone shares one IP. |
| `PROCERA_TRUSTED_PROXY_HOPS` | `--trusted-proxy-hops` | `1` | No | How many reverse proxies in front of the server append to `X-Forwarded-For`. The client address is taken that many entries from the end, so earlier (client-supplied) entries are ignored. Use `0` if the server is reached directly with no proxy. |
| `PROCERA_FILES_DIR` | (none) | a `procera-files` folder in the system temp directory (the container image sets `/var/lib/procera/files`) | Yes, in practice | Where uploaded files are stored (encrypted). Without it, files can be lost on restart, and the console warns about it. Local disk is the only storage today; S3-compatible storage is not built yet. |
| `PROCERA_CLAMD_ADDR` | (none) | none | Recommended | ClamAV `clamd` address (`host:port`, e.g. `clamav:3310`). Without it, uploads are checked only for the EICAR test file. |
| `PROCERA_LOG` | (none) | `info` | No | Log filter in `tracing` EnvFilter syntax, e.g. `warn`, `debug`, `info,procera_notify=debug`. |

### `procera migrate`

| Variable | Flag | Default | Required | Purpose |
|---|---|---|---|---|
| `PROCERA_MIGRATOR_URL` | `--migrator-url` | none | Yes | PostgreSQL URL for the `procera_migrator` role. |
| `PROCERA_LOG` | (none) | `info` | No | As above. |

### `procera healthcheck`

| Variable | Flag | Default | Purpose |
|---|---|---|---|
| `PROCERA_BIND` | `--bind` | `127.0.0.1:8080` | Address of the running server to probe. |

### Database roles

Run `deploy/sql/00-roles.sql` once per database, as a superuser, before the first migration. It creates:

- `procera_migrator`: owns the schema and runs migrations. Never used by the running server.
- `procera_app`: the runtime role. Not an owner, no `BYPASSRLS`, data changes only, so row-level security
  always applies.
- `procera_discovery`: cannot log in. Owns only a few cross-workspace lookup functions that return ids.

It also creates the `citext` and `ltree` extensions. Set the two login roles' passwords yourself (the
Compose stack does this from `deploy/.env`).

### Endpoints for monitoring

| Path | Answers |
|---|---|
| `GET /healthz` | `{"status":"ok"}` while the process is up. |
| `GET /readyz` | `status` (`ready` or `not_ready`), `database` (`ok`, `unavailable` or `none`) and the cache backend and health. A degraded cache stays ready. |
| `GET /version` | Name, version and server time. |

### Variables used by the Compose files only

`deploy/compose.yml` also reads these from `deploy/.env`. The server never sees them directly.

| Variable | Default | Purpose |
|---|---|---|
| `POSTGRES_PASSWORD` | none (required) | Postgres superuser password. |
| `PROCERA_MIGRATOR_DB_PASSWORD` | none (required) | Password set for `procera_migrator` on first start; builds `PROCERA_MIGRATOR_URL`. |
| `PROCERA_APP_DB_PASSWORD` | none (required) | Password set for `procera_app` on first start; builds `PROCERA_DATABASE_URL`. |
| `PROCERA_HTTP_PORT` | `8080` | Host port (optionally `ip:port`) for the web container's HTTP. |
| `PROCERA_HTTPS_PORT` | `8443` | Host port for HTTPS, used when `PROCERA_SITE_ADDRESS` is a domain. |
| `PROCERA_SITE_ADDRESS` | `:80` | Caddy site address. A domain name turns on automatic HTTPS certificates. |
| `PROCERA_IMAGE`, `PROCERA_WEB_IMAGE` | `procera:local`, `procera-web:local` | Image names to build or use. |
| `MAILPIT_UI_PORT` | `8025` | Mailpit's web inbox, on 127.0.0.1 (with the `mail` profile). |
| `COMPOSE_PROFILES` | none | Optional services to start: `cache`, `scan`, `mail`. |

## 2. Settings in the app

### System console

System administrators sign in and open **System console** (from **Choose a workspace**, or `/system`).
Two-step verification is required. Changes are recorded in **System audit**.

| Page | What you can change |
|---|---|
| **Workspaces** | Create workspaces (**New workspace**: name, slug, configuration profile, owner email), **Make read-only**, **Lock**, **Reactivate**, **Schedule deletion** (with a grace period), and **Edit quotas** per workspace (member limit, storage limit). |
| **Quotas & usage** | Seats and storage per workspace, and **Defaults by profile**: the member and storage limits copied into each new workspace. The member limit is enforced when people are invited. |
| **Email & platform** | **Email delivery**: SMTP host, port, **Encryption**, username, password, from address, sender name and reply-to, plus **Send test email**. Changes apply without a restart. If `PROCERA_SMTP_URL` is set, it wins and the form is replaced by a notice. **Platform basics**, **File storage and malware scanning**: shown as configured on the server, with where each value comes from. |
| **Platform health** | Status checks for the installation. |
| **System audit** | The system-plane audit log. |

**Platform floors** set the minimum every workspace must meet. A workspace can be stricter but never looser:
a looser value is rejected when saved, and existing looser values are clamped and shown as
**Platform minimum** in workspace Settings. Keys with floors today:
`auth.mfa.required_roles`, `auth.mfa.allowed_methods`, `auth.password.min_length`,
`auth.password.breach_check`, `auth.session.idle_timeout`, `auth.session.absolute_timeout`,
`auth.stepup.window`, `api.tokens.max_ttl`, `access.admin_content_access_log`,
`retention.governance_record.keep_for`, `files.max_size`, `files.scan.required`.
There is no console page for floors yet: set them through the system API
(`GET /sysapi/v1/settings`, `PUT /sysapi/v1/settings/{key}` with `{"value": …, "reason": "…"}`), signed in
as a system administrator.

### Workspace Settings

Workspace owners and admins open **Admin › Settings**. Each setting shows its effective value and where it
comes from (default, profile, workspace, department or platform minimum). The categories:

| Category | Covers |
|---|---|
| **General & branding** | Name, logo, colours, language, time zone and the words Procera uses |
| **Sign-in & security** | Passwords, two-step verification, session length and admin access logging |
| **People & directory** | Who can join, what happens when someone leaves, positions |
| **Documents & reviews** | Who must review before publishing, review cycles, numbering |
| **Acknowledgements & announcements** | When people must re-acknowledge, campaign reminders and escalation, announcement rules |
| **Notifications** | Which channels Procera uses, digests and quiet hours |
| **Data, retention & files** | Upload limits, malware scanning, and how long records are kept |

Some settings can be overridden per department. The configuration profile chosen when the workspace was
created only sets the starting values. The full catalogue is in
[docs/20](../20-WORKSPACE-CONFIGURATION.md).

## 3. Security notes

### Reverse proxy and TLS

The `procera` server speaks plain HTTP and does not serve the web app. Put a reverse proxy in front that:

- terminates TLS (HTTPS is required outside local trials; see cookies below);
- serves the built web app (`npm run build` in `web/` produces `web/dist`) and falls back to `index.html`
  for unknown paths, because the app uses browser history routing;
- forwards `/api/`, `/sysapi/`, `/healthz`, `/readyz` and `/version` to the server, on the **same origin**
  as the web app;
- **replaces** `X-Forwarded-For` with the real client address. Procera trusts the first address in that
  header for sign-in rate limits and audit records, so a proxy that appends to a client-supplied header
  lets clients pick their own address. Don't expose the server port directly.

The Compose stack's `web` container (Caddy) does all of this. If you run your own proxy, a minimal
**Caddy** setup:

```caddyfile
procera.example.com {
	encode zstd gzip
	@api path /api/* /sysapi/* /healthz /readyz /version
	handle @api {
		reverse_proxy 127.0.0.1:8080
	}
	handle {
		root * /srv/procera/web/dist
		try_files {path} /index.html
		file_server
	}
}
```

Caddy gets the certificate automatically, and it replaces client-sent `X-Forwarded-For` unless you
configure `trusted_proxies`.

A minimal **nginx** setup (certificates from your usual tooling):

```nginx
server {
    listen 443 ssl;
    http2 on;
    server_name procera.example.com;
    ssl_certificate     /etc/ssl/procera/fullchain.pem;
    ssl_certificate_key /etc/ssl/procera/privkey.pem;

    root /srv/procera/web/dist;
    location ~ ^/(api|sysapi)/ {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $remote_addr;   # replace, don't append
        proxy_set_header X-Forwarded-Proto $scheme;
    }
    location ~ ^/(healthz|readyz|version)$ {
        proxy_pass http://127.0.0.1:8080;
    }
    location / {
        try_files $uri /index.html;
    }
}

server {
    listen 80;
    server_name procera.example.com;
    return 301 https://$host$request_uri;
}
```

Set `PROCERA_PUBLIC_URL=https://procera.example.com` to match.

### Cookies and sessions

- The refresh cookie is `HttpOnly`, `SameSite=Strict`, `Secure` and named with the `__Host-` prefix, so
  browsers send it only over HTTPS, only to the exact host, and never on cross-site requests. Cookie
  endpoints also require a custom request header, which blocks cross-site form posts.
- `PROCERA_INSECURE_COOKIES=true` drops `Secure` and the prefix so a plain-HTTP setup works. Use it only
  for local development and tests.
- Session length, idle timeout and the step-up window are workspace settings (**Sign-in & security**),
  bounded by platform floors.

### Master key

- Generate it with `procera keygen`, store it in a secret manager, and back it up apart from database
  backups. **Losing it makes the encrypted data unreadable**: workspace keys, stored files, two-step
  secrets and the saved SMTP password.
- Don't change it on an existing installation: there is no root-key rotation command yet. Each workspace's
  key can be rotated with **Rotate key** under **Admin › Encryption**.
- External key management (KMS/HSM) is designed but not built yet; the key is read from the environment.

### SMTP TLS modes

| Where | Mode | Setting |
|---|---|---|
| System console | **STARTTLS (required)** | Upgrades to TLS and stops delivery if the server doesn't offer it. Usually port 587. Recommended. |
| System console | **Implicit TLS** | TLS from the first byte. Usually port 465. |
| System console | **None** | Unencrypted. Accepted only when the host is `localhost`, `127.0.0.1` or `::1` (a relay on the same machine). |
| `PROCERA_SMTP_URL` | `smtp://…:587?tls=required` | STARTTLS required. |
| `PROCERA_SMTP_URL` | `smtps://…:465` | Implicit TLS. |
| `PROCERA_SMTP_URL` | `smtp://host:port` with no `tls` parameter | Unencrypted, with no host restriction. Use only for a test inbox such as Mailpit or a relay on a private network. |

The saved SMTP password is encrypted under the system key and never shown again. A password in
`PROCERA_SMTP_URL` lives wherever your environment is stored, so protect that file.

### Other hardening

- The container image runs as a non-root user on a distroless base (no shell).
- Keep Postgres off the public network. The Compose stack publishes only the `web` container.
- Turn on ClamAV (`PROCERA_CLAMD_ADDR`) before people upload files.
- Keep `/readyz` internal if you don't want its database and cache status visible; the bundled Caddyfile
  forwards it.
