# Fraud Platform — web UI

Nuxt 4 (Vue 3, TypeScript strict) + Nuxt UI v4, served with SSR. The Nuxt server also acts as a **BFF**:

```
browser ──cookie──▶ Nuxt server (BFF) ──Bearer JWT──▶ gateway (Traefik) ──▶ core-api / rule / graph / ml / llm / ingest
```

* `server/api/auth/{login,logout,refresh}` call the gateway and keep the access and refresh tokens in a **sealed,
  httpOnly, SameSite=strict** session cookie (nuxt-auth-utils). The browser never sees a token.
* `server/api/[...path].ts` proxies `/api/<x>` to `${NUXT_API_BASE_URL}/api/v1/<x>`. It adds the bearer token,
  refreshes it proactively (and once more on a 401, as a single-flight refresh), streams responses (SSE, downloads), and
  passes multipart uploads through. It only forwards allow-listed headers and never forwards upstream `Set-Cookie`.
  Traversal and `/api/v1/...` paths are rejected.
* CSRF protection: SameSite=strict, plus a required `x-requested-with: fraud-web` header and an Origin check on
  mutating requests.
* Tokens are refreshed single-flight per Node process. With several web replicas, use sticky sessions.

## Run

```bash
npm ci
npm run dev:mock          # full UI against the built-in mock API (no backend) → http://localhost:3000
npm run dev               # against a gateway: NUXT_API_BASE_URL=http://localhost:8080
```

Mock users (any password except `wrong`): `owner@demo.local` (tenant admin), `analyst@demo.local`,
`approver@demo.local`, `viewer@demo.local`, `admin@fraud.local` (platform admin; no project access by design).

| Env | Default | Purpose |
|---|---|---|
| `NUXT_API_BASE_URL` | `http://localhost:8080` | gateway |
| `NUXT_SESSION_SECRET` | — (required in prod, ≥ 32 chars) | seals the session cookie |
| `NUXT_SESSION_SECURE` | `false` | `true` behind TLS (Secure cookie) |
| `NUXT_API_TIMEOUT_MS` | `30000` | upstream timeout (streams exempt) |
| `MOCK_API` / `NUXT_MOCK_API` | — | `1` = serve fixtures in-process |

## Verify

```bash
npm run lint && npm run typecheck && npm test
python3 i18n/messages.py --check        # every t('…') key exists in id + en
npm run build && scripts/smoke-mock.sh  # built server + mock: BFF security checks + every page SSR 200
docker build -t fraud-platform/web .
```

## Layout

| Path | What |
|---|---|
| `app/pages/` | `/login`, `/projects(/new)`, `/tenant/*`, `/admin/tenants`, and project pages `/p/[pid]/…` (dashboard, events, cases, rules, rulesets, reference-lists, formulas, proposals, ml/*, graph, llm/*, data-sources, field-catalog, settings, members, audit) |
| `app/components/rules/` | recursive condition tree, operand editor (const/field/formula/ref/hist), velocity/statistic, reference and graph editors, test/backtest panels, version diff, maker–checker buttons |
| `app/components/ml/` | JSON-Schema-driven parameter forms for algorithm plugins, train modal, model metric charts |
| `app/components/graph/` | Cytoscape (fcose) canvas |
| `app/components/datasources/` | mapping editor + transform pipeline editor |
| `shared/` | framework-free logic shared with tests: API types, rule DSL types, UI model ⇄ DSL serialiser, schema-form engine, problem+json parser, diff |
| `server/` | BFF routes and utilities, `mock/` (stateful demo API) |
| `i18n/messages.py` | single source for the Indonesian (default) and English strings → `i18n/locales/*.json` |
