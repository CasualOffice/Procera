# ADR-0020 — JWT access tokens with rotating refresh tokens (no server-side sessions)

Status: **Accepted** · Date: 2026-10-11 · Deciders: Product owner ("use JWT; don't complicate things"), Tech lead, Security lead · Supersedes the server-side session design in docs/06 §3.2 (v0.2).

## Context

The v0.2 design used opaque server-side sessions in a cookie, with a synchronizer CSRF token on every state-changing request. The product owner asked for JWT and less complexity. The security requirements still apply:
- short-lived credentials;
- immediate effect of deactivation (SEC-04);
- refresh-token theft detection (SEC-14);
- no CSRF exposure.

## Decision

1. **Access token = JWT, HS256, 10 min TTL**, sent as `Authorization: Bearer`. Claims:
   - `iss=procera`, `aud=procera-api`;
   - `sub` (user), `sid` (sign-in family), `plane`, `ws` (active workspace, authoritative);
   - `amr`, `auth_time`, `tv` (user token version), `typ=access`;
   - `iat`, `exp`, `jti`.

   The algorithm is pinned (no `alg` negotiation, so `none` and alg-confusion are rejected). The signing key is derived from the master key (`HMAC(root, "jwt-access-v1")`), so there's no extra secret to manage.
2. **Refresh token = opaque 256-bit random value**, stored only as SHA-256, in an `HttpOnly; Secure; SameSite=Strict; __Host-` cookie.
   - It is **rotated on every use**. Presenting an already-rotated token revokes the whole sign-in family and is audited as `security.token_reuse`.
   - Sliding expiry is 12 h; absolute expiry is 30 d.
3. **CSRF:** normal API calls use the Bearer header, which browsers never attach automatically, so they need no CSRF token. The only cookie endpoints (`/auth/refresh`, `/auth/logout`) require `SameSite=Strict` **and** the custom header `X-Requested-With: procera`, which cross-site forms can't send.
4. **Revocation:**
   - Logout revokes the family.
   - Sign-out-everywhere and deactivation revoke all families **and bump `users.token_version`**.
   - Every request compares the JWT's `tv` with the user's current status and version through a ≤ 30 s cache. Revocation therefore takes effect within seconds, and instantly on the node that performed it.
5. **MFA:** a password success returns a 5-minute `typ=mfa` JWT that is good only for `/auth/mfa`. TOTP (RFC 6238, replay-protected) or a single-use recovery code completes sign-in, giving `amr=[pwd, otp]`.

## Alternatives considered

- **Server-side sessions plus synchronizer CSRF tokens:** more moving parts in every request. Rejected by the owner.
- **Access JWT in a cookie:** reintroduces CSRF on every endpoint.
- **EdDSA / RS256:** useful when third parties must verify tokens. Not needed while only Procera verifies its own tokens. Revisit if a separate resource server appears (e.g. connectors), and add a JWKS endpoint then.
- **Long-lived access tokens without a version check:** deactivation would lag by the token lifetime.

## Consequences

- One small per-request lookup (`status`, `token_version`), cached for 30 s.
- The web client keeps the access token **in memory only** and calls `/auth/refresh` on load and before expiry.
- Implemented in `crates/identity` and `apps/procera/src/http/auth.rs`. Tests: 10 unit/integration tests plus E2E-R1-AUTH-01…07.
