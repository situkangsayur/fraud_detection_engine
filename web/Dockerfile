# Fraud Platform web (Nuxt 4 SSR + BFF). Classic-builder compatible (no BuildKit-only syntax).
# Nuxt 4.5 / Vitest 5 require Node >= 22.19.
ARG NODE_IMAGE=node:22-alpine

# ---- deps + build ------------------------------------------------------------
FROM ${NODE_IMAGE} AS build
WORKDIR /app
ENV NUXT_TELEMETRY_DISABLED=1
COPY package.json package-lock.json .npmrc ./
RUN npm ci --ignore-scripts --no-audit --no-fund
COPY . .
RUN npx nuxt prepare && npm run build

# ---- runtime -----------------------------------------------------------------
FROM ${NODE_IMAGE} AS runtime
LABEL org.opencontainers.image.title="fraud-platform-web" \
      org.opencontainers.image.authors="Hendri Karisma <situkangsayur@gmail.com>"
ENV NODE_ENV=production \
    NUXT_TELEMETRY_DISABLED=1 \
    HOST=0.0.0.0 \
    PORT=3000
WORKDIR /app
# .output is self-contained (Nitro bundles its server dependencies).
COPY --from=build --chown=node:node /app/.output ./.output
USER node
EXPOSE 3000
HEALTHCHECK --interval=15s --timeout=5s --start-period=15s --retries=5 \
  CMD wget -qO- http://127.0.0.1:3000/healthz || exit 1
# Browsers send every cookie of the host, whatever the port. Other apps on the same IP (Keycloak, Supabase…)
# can push the Cookie header past Node's 16 KB default, and Node then drops the connection ("premature close").
CMD ["node", "--max-http-header-size=65536", ".output/server/index.mjs"]
