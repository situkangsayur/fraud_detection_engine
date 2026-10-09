# fraud-simulator

Generates realistic synthetic activity for the fraud platform and pushes it through the **public gateway
API only**, the same way a client system plus an operator would.

## What it does

1. Logs in and makes sure tenant `SIM_TENANT` (default `demo`) has a tenant-admin session. If
   `SIM_TENANT_ADMIN_EMAIL`/`SIM_TENANT_ADMIN_PASSWORD` already work they are used. Otherwise the platform
   admin creates the tenant, or (re)provisions that tenant admin.
2. Creates or reuses 4 projects with stage templates: `checkout` (pre_payment), `post-payment`, `returns` and
   `promo`.
3. Creates a webhook data source `sim-webhook` per project in a **custom, non-canonical shape**: Indonesian
   field names, a nested `pengguna` object, `dd/mm/yyyy` timestamps, `1.500.000,00` amounts, and raw card and
   account numbers. It then registers and activates the mapping from `simulator/shape.py`, which hashes and
   drops the raw PAN and account numbers.
4. Generates `SIM_DAYS` of deterministic (`SIM_SEED`) behaviour for `SIM_CUSTOMERS` customers (scaled per
   project), with injected typologies:

| project | typologies |
|---|---|
| checkout | carding rings (card testing → big hits, many stolen cards per device, BIN/IP-country mismatch), account takeover (failed logins → new device → credential/phone/address change → high-value orders), system breach (night-time burst through one `id_klien_api` across many accounts) |
| post-payment | bank/e-wallet account takeover (new beneficiary + drain payouts), money mules (fan-in transfers → fast payouts) |
| returns | refund abuse rings (serial fast returns, shared device, address variants) |
| promo | promo farms (multi-accounts sharing devices, address and phone variants, same voucher, cashback payouts) |

5. Sends events **chronologically** in batches of 500 to `POST /api/v1/ingest/sim-webhook/batch` with the
   source API key. The first 70 % of the window goes in as `load_only` (history for features and training) and
   the rest as `score`.
6. Posts delayed, partial ground truth to `POST /api/v1/projects/{pid}/labels`:
   * 60 % of fraud events older than 7 days;
   * about 3 % legit reviews;
   * 70 % of fraud-ring customers marked `fraud` (which feeds graph proximity).
7. Prints a summary. Re-runs are idempotent: ingest and labels are skipped when data already exists, unless
   `--force` is passed.

## Usage

```bash
docker compose --profile seed run --rm simulator                  # full run (env from .env)
uv run python -m simulator stats --customers 2000 --days 60       # generation stats only, no API
uv run python -m simulator export --project returns --out data/returns.csv --delimiter ';' --end 2026-09-20
```

`export` writes the custom shape (CSV with dotted headers such as `pengguna.id`, or JSONL) plus ground-truth
columns `is_fraud`/`jenis_fraud`. Use it to try the **file import path**: Data Sources → upload → inspect →
the suggested mapping (including the `label` section) → activate → start a job.

Env: `GATEWAY_URL`, `ADMIN_EMAIL`, `ADMIN_PASSWORD`, `SIM_TENANT`, `SIM_TENANT_ADMIN_EMAIL`,
`SIM_TENANT_ADMIN_PASSWORD` (default `ADMIN_PASSWORD`), `SIM_CUSTOMERS`, `SIM_DAYS`, `SIM_SEED`, `SIM_END`.

Tests: `uv run pytest` (determinism, typology presence and rates, chronology, no ground-truth leakage, Luhn
PANs, mapping coverage, label plan, export, and a full flow against a mocked gateway).
