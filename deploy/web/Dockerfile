# Procera web app + reverse proxy. The procera server serves only the API (/api, /sysapi, /healthz,
# /readyz, /version); this image builds web/dist and serves it with Caddy, proxying the API paths
# to the server so the browser sees one origin (the refresh cookie is SameSite=Strict).
# Build context: the repository root (see deploy/compose.yml).
FROM node:22-bookworm-slim AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM caddy:2-alpine
COPY deploy/web/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /web/dist /srv
