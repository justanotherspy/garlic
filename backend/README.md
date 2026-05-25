# garlic-backend 🧄🛰️

A small, self-hostable HTTP service that stores garlic's time-tracking **state**
in Redis, so every Claude Code agent across every machine and environment can
share one set of daily totals. Point each client at the backend with two
environment variables:

```bash
export GARLIC_URL="https://garlic.example.com"
export GARLIC_TOKEN="<one of the server's configured tokens>"
```

The CLI keeps working with local state when these are unset; the backend is the
shared-state path going forward.

## Design in one paragraph

The backend owns **state** (the part that must be shared); the **config** that
governs time accounting stays with the client and is sent on each request. This
mirrors garlic's existing `engine(state, config)` split exactly — the engine
logic (gap accounting, thresholds, daily rollover, bedtime) is ported
faithfully from the Python `engine.py`/`state.py` into Rust. The **server clock
is authoritative**: it timestamps every event and computes the day boundary in
its own timezone (`TZ`), so clients in different timezones still share one
consistent "day" and can't drift via clock skew. Each request's read-modify-write
runs under a **per-namespace Redis lock**, so concurrent agents never race on
the accumulated total — safe even across multiple backend replicas.

## Quick start (Docker)

```bash
cd backend
cp .env.example .env
# edit .env: set GARLIC_AUTH_TOKENS (e.g. `openssl rand -hex 32`) and TZ
docker compose up -d --build

curl -s localhost:8080/health
# {"redis":"ok","status":"ok","version":"0.1.0"}
```

For automatic HTTPS, set `GARLIC_DOMAIN` in `.env`, point its DNS at the host,
and start with the `tls` profile (Caddy fronts the backend and manages
Let's Encrypt certificates):

```bash
docker compose --profile tls up -d --build
```

## Configuration (environment variables)

| Variable             | Required | Default        | Description                                                        |
| -------------------- | -------- | -------------- | ------------------------------------------------------------------ |
| `GARLIC_REDIS_URL`   | yes\*    | —              | Redis connection string, e.g. `redis://host:6379` or `rediss://…`. |
| `REDIS_URL`          | —        | —              | Fallback if `GARLIC_REDIS_URL` is unset.                           |
| `GARLIC_AUTH_TOKENS` | yes      | —              | Comma-separated accepted bearer tokens; each = one namespace.      |
| `GARLIC_BIND`        | no       | `0.0.0.0:8080` | Listen address.                                                    |
| `TZ`                 | no       | `UTC`          | Timezone defining the daily reset boundary for all clients.        |
| `RUST_LOG`           | no       | `info`         | Log filter (`error`/`warn`/`info`/`debug`/`trace`).               |

\* `GARLIC_REDIS_URL` or `REDIS_URL` must be set. The service refuses to start
if no auth tokens are configured (fail-closed).

### HTTPS

The binary serves plain HTTP and is designed to run behind a TLS-terminating
reverse proxy (the bundled Caddy profile, or your platform's load balancer).
This keeps the image small and the deployment flexible. Clients still speak
HTTPS to the proxy — the public API is HTTPS end-to-end.

## Authentication & multi-user

Send the token as a bearer credential:

```
Authorization: Bearer <token>
```

The namespace for a token is `SHA-256(token)` in hex, so raw tokens never appear
in Redis keys (`garlic:state:<namespace>`). Two agents that should share state
use the **same** token; two people who should not share state use **different**
tokens. Add more tokens to `GARLIC_AUTH_TOKENS` to onboard more users.

## API

Base path for state operations is `/v1`. All `/v1/*` endpoints require the
bearer token. Errors are JSON: `{"error":"<message>"}` with an appropriate
status (`401` unauthorized, `400` bad body, `503` busy/Redis down, `500` other).

### Meta (no auth)

| Method | Path       | Description                                              |
| ------ | ---------- | -------------------------------------------------------- |
| `GET`  | `/health`  | `200` when Redis is reachable, `503` otherwise.          |
| `GET`  | `/version` | `{"name":"garlic-backend","version":"x.y.z"}`.           |

### State

Mutating endpoints accept an optional JSON **config** body. Any omitted field
falls back to garlic's default, so `{}` (or no body at all) is valid. The four
`/v1/events/*` endpoints additionally accept a `session_id` so the backend can
attribute intervals to the session that produced the event (it defaults to
`"default"` when omitted). The server stamps every interval boundary with its
own clock — clients never send timestamps.

```json
{
  "max_prompt_gap_minutes": 40,
  "max_generation_minutes": 120,
  "reset_hour": 2,
  "nudge_thresholds_minutes": [30, 60, 90, 120, 150, 180, 210, 240],
  "session_id": "abc123"
}
```

| Method | Path                         | Body              | Purpose                                                            |
| ------ | ---------------------------- | ----------------- | ------------------------------------------------------------------ |
| `GET`  | `/v1/state?reset_hour=N`     | —                 | Current state (applies daily rollover). `reset_hour` defaults `2`. |
| `POST` | `/v1/events/session-start`   | config + session  | Open the session's user-thinking interval (SessionStart hook).     |
| `POST` | `/v1/events/prompt`          | config + session  | Close user / open agent interval; report crossed threshold + bedtime. |
| `POST` | `/v1/events/stop`            | config + session  | Close agent interval (clamped to `max_generation_minutes`) / open user. |
| `POST` | `/v1/events/session-end`     | config + session  | Finalize the session's in-flight interval and drop its cursor.     |
| `POST` | `/v1/ignore`                 | config + `set?`   | Toggle (or set, via `"set": true/false`) the daily nudge pause.    |
| `POST` | `/v1/reset`                  | config            | Zero today's timer (history preserved).                            |

Every state endpoint returns the same envelope. `accumulated_minutes` is the
**union** of all intervals' wall-clock (concurrent sessions are not
double-counted), and `intervals`/`open` carry the per-session agent/user spans:

```json
{
  "state": {
    "date": "2026-05-22",
    "accumulated_minutes": 123.4,
    "last_event_time": 1779495417.55,
    "nudges_given": [30, 60],
    "ignored": false,
    "bedtime_nudge_given": false,
    "history": [{ "date": "2026-05-21", "minutes": 210.0 }],
    "intervals": [
      { "session_id": "abc123", "kind": "user", "start": 1779495000.0, "end": 1779495180.0 },
      { "session_id": "abc123", "kind": "agent", "start": 1779495180.0, "end": 1779495417.55 }
    ],
    "open": []
  },
  "crossed_threshold": 60,
  "bedtime": false
}
```

`crossed_threshold` (the highest newly-crossed nudge threshold, or `null`) and
`bedtime` are only meaningful for `POST /v1/events/prompt`; other endpoints
return `null`/`false`. Following garlic's hook semantics, the backend still
records that a threshold was crossed even when `ignored` is true — the client
decides whether to surface a nudge based on `state.ignored`. The nudge wording
(`gentle`/`firm`/`spicy`) stays entirely client-side.

### Example

```bash
URL=http://localhost:8080
TOK="your-token"

curl -s -X POST "$URL/v1/events/session-start" \
  -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' \
  -d '{"reset_hour":2}'

curl -s -X POST "$URL/v1/events/prompt" \
  -H "Authorization: Bearer $TOK" -H 'Content-Type: application/json' \
  -d '{"nudge_thresholds_minutes":[30,60,90,120],"max_prompt_gap_minutes":40,"session_id":"abc123"}'

curl -s "$URL/v1/state?reset_hour=2" -H "Authorization: Bearer $TOK"
```

## Development

```bash
cargo test      # unit + API tests (no Redis needed — uses an in-memory store)
cargo clippy --all-targets
cargo fmt

# Run against a local Redis:
redis-server --port 6379 &
GARLIC_REDIS_URL=redis://127.0.0.1:6379 \
GARLIC_AUTH_TOKENS=dev-token \
cargo run
```

### Layout

- `model.rs` — `State`/`TimeConfig` (field names match garlic's `state.toml`).
- `engine.rs` — pure time-accounting logic + the `Clock` trait (faithful port).
- `store.rs` — `Store` (Redis + in-memory) and the locked read-modify-write.
- `auth.rs` — bearer-token validation and namespace derivation.
- `routes.rs` — axum handlers; `config.rs` — env config; `main.rs` — server.

## Data & durability

State is one JSON value per namespace at `garlic:state:<sha256(token)>`. Lock
keys (`garlic:lock:<…>`, 10s TTL) are transient. Configure Redis persistence
(RDB/AOF) to your taste — the compose file enables periodic RDB snapshots.
