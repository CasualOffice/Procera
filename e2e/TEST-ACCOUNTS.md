# Test and demo accounts

These are local, throwaway credentials for the seeded demo and for manual testing. They are **not** production accounts, and they only exist after `scripts/demo.sh` has seeded the database.

## Local demo (`./scripts/demo.sh`)

- **Web:** http://127.0.0.1:5173 (`5173 + DEMO_OFFSET`)
- **API:** http://127.0.0.1:18500 (`18500 + DEMO_OFFSET`)
- **Workspace:** Acme (`acme`)
- **Password for every account:** `correct horse battery staple`

Sign-in is **password only**: the demo turns off two-step verification for workspace roles (`auth.mfa.required_roles = []`).

| Email | Name | Role | Department / title |
|---|---|---|---|
| `olivia@acme.test` | Olivia Owner | **workspace owner** | — |
| `ema@acme.test` | Ema Rossi | workspace.admin | People, People partner |
| `ben@acme.test` | Ben Baker | reviewer | Operations, Head of Compliance |
| `cara@acme.test` | Cara Chen | member | Operations, Support lead |
| `dev@acme.test` | Dev Patel | member | Operations, IT engineer |
| `finn@acme.test` | Finn Walsh | member | Finance, Finance analyst |
| `gia@acme.test` | Gia Nguyen | member | Operations, Office manager |
| `nina@acme.test` | — | invited only (pending) | — |

### System admin (`/sysapi`)

- **Email:** `root@acme.test` (Riley Root)
- **Password:** the same as above.
- **Two-step verification:** required. Use one of the recovery codes that `scripts/demo-seed.mjs` prints at the end of the seed (`system_recovery_codes`). They change on every reseed.

## Automated E2E suite (`./scripts/test-all.sh`)

The Playwright packs create their own users per test with `seedUser()` in `e2e/fixtures/accounts.ts`:

- **Emails:** unique `<tag>-<timestamp>-<random>@acme.test`.
- **Password:** `PW` = `correct horse battery staple`.
- **API base:** `PROCERA_BASE_URL`, default `http://127.0.0.1:18080` (`+ PORT_OFFSET`).

No fixed accounts are shared between tests.

Keep this file in sync with `scripts/demo-seed.mjs` whenever the seeded people or the password change.
