# Mirror Module Boundaries

Mirror uses documented module boundaries first, not crate-enforced boundaries.
The goal is isolation and testability without crate explosion.

## Backend Dependency Direction

Allowed high-level flow:

```text
Actix route/middleware
  -> feature module function
  -> sqlx/storage/jobs/external tool wrapper
```

Feature logic must not depend on Actix request/response types. Handlers adapt
HTTP into typed inputs and typed module outputs into HTTP responses.

## Backend Module DAG

```text
config
  -> db
  -> ids/time

auth -> audit, limits
assets -> storage, jobs, audit, sync
uploads -> storage, assets, jobs, limits, audit
media -> storage, jobs, audit
albums -> assets, audit, sync
shares -> assets, limits, audit
search -> assets, people, models
people -> assets, models, jobs, audit, sync
models -> storage, jobs, audit
exports -> assets, storage, jobs, audit
backups -> storage, jobs, audit
jobs -> db
limits -> db
sync -> db
```

Rules:

- `storage` does not depend on `assets`, `uploads`, or HTTP.
- `jobs` stores and leases jobs; job handlers live near the feature they execute.
- `audit` is append-only from feature modules.
- `limits` knows about action keys and buckets, not HTTP handlers.
- `sync` records first-party cache-refresh events, not public webhooks.
- `media` and `ml-worker` use explicit external-tool/model wrappers.

## Pure/Value Modules

Prefer small pure modules for invariants:

- Public IDs and UUIDv7 parsing/formatting.
- Storage keys and content-addressed paths.
- BLAKE3 hash wrappers.
- Share token generation/hashing.
- Session/device token generation/hashing.
- Timeline cursor encoding/decoding.
- Trusted proxy/client IP extraction.
- Rate-limit key construction and decisions.
- Upload completion state machine.

These modules should be easy to unit test without Postgres, Actix, OpenDAL, or
Android/Web clients.

## Interfaces That May Be Traits

Use traits only where v1 has a concrete reason:

- Storage backend.
- ML runtime.
- Clock/time provider for tests.
- ID/token generator for tests.
- External media/model command runner for tests.

Do not create generic repository traits, use-case traits, service containers, or
domain-event interfaces.

## Frontend Boundaries

Web:

- `src/web/src/api`: manual API client and DTOs.
- `src/web/src/routes`: TanStack Router route definitions.
- `src/web/src/features/<feature>`: feature UI/state.
- Route components call feature hooks; feature hooks call API client.
- Do not let UI components construct raw endpoint URLs.

Android:

- Feature packages may use `ui`, `data`, and `domain` only where helpful.
- `MirrorApi` owns HTTP, auth headers, error decoding, and upload streaming.
- Repositories coordinate Room and `MirrorApi`.
- ViewModels expose immutable `StateFlow` state.
- Composables do not perform network or database work directly.

## Boundary Enforcement

Early enforcement:

- Code review and task checklist.
- Unit tests for pure modules.
- `rg`/script checks once paths exist to catch forbidden imports such as Actix
  types in feature logic.

Later enforcement, only if needed:

- Split selected pure/core modules into separate crates.
- Add import-boundary CI scripts.

Do not split crates preemptively.
