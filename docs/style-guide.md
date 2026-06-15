# Mirror Style Guide

This guide controls code shape and documentation style. It exists to keep the
codebase small, explicit, testable, and resistant to implementation drift.

## Overall Style

- Prefer simple, direct code over clever abstractions.
- Prefer explicit data flow over implicit global state.
- Prefer named types for important invariants over raw strings/integers.
- Prefer boring functions over frameworks, macros, builders, or traits unless
  they remove real complexity.
- Prefer small modules with clear ownership over broad utility modules.
- Prefer deleting unused code over keeping speculative hooks.
- Do not add generic "common", "helpers", or "utils" modules unless the helper
  has a precise domain name and at least two real call sites.
- Do not redefine the same Rust free-function name across files. Move reused
  behavior into a precise domain or test-support module and import it. Required
  binary `main` functions and `impl`/trait methods are excluded.
- Do not introduce clean architecture, onion architecture, service containers,
  generic repositories, domain event buses, or actor systems.

Good code should read like a sequence of domain decisions:

```text
parse request -> validate invariant -> run query/storage op -> enqueue job -> return typed result
```

## Comments And Doc Comments

Use comments to preserve decisions, not to narrate syntax.

Doc comments are required for:

- Public modules.
- Public structs/enums/functions.
- Types that encode invariants.
- Functions that perform I/O, persistence, crypto, auth, deletion, backup,
  upload promotion, token handling, rate limiting, or media/ML processing.

Doc comments should mention:

- What invariant the item owns.
- What side effects it has.
- What it deliberately does not do.
- Relevant footguns or caveat IDs from `docs/caveats.md`.

Doc comments should not repeat obvious type information.

Good:

```rust
/// Promotes a verified staged upload into immutable original storage.
///
/// This writes object storage before committing asset rows, so callers must
/// run orphan cleanup/integrity recovery described by C001 if the DB commit
/// fails after promotion.
pub async fn promote_verified_upload(...) -> Result<PromotedOriginal, Error> { ... }
```

Bad:

```rust
/// Promotes upload.
pub async fn promote_verified_upload(...) -> Result<PromotedOriginal, Error> { ... }
```

Inline comments are appropriate for non-obvious ordering, security rationale,
or recovery behavior. They are not needed for plain assignments, simple
branches, or obvious constructor calls.

## Rust Style

- Keep feature logic free of Actix request/response types.
- Keep SQL close to the module that owns the data.
- Use `sqlx` directly; do not add repository traits for ordinary DB access.
- Use traits only for storage backend, ML runtime, clock/id generators in tests,
  and external command runners.
- Use newtypes for security/data boundaries:
  - `SessionTokenHash`
  - `DeviceTokenHash`
  - `ShareTokenHash`
  - `StorageKey`
  - `OriginalHash`
  - `TimelineCursor`
  - `TrustedClientIp`
- Avoid `String`ly typed actions and states when an enum can encode valid
  values.
- Avoid broad `anyhow::Error` in core feature logic; prefer feature-specific
  errors that can map into API errors.
- Do not use `unwrap`/`expect` in request, job, storage, auth, upload, backup,
  or media paths unless the invariant is local and impossible to violate. If
  used, the message must explain the invariant.
- Avoid cloning large data or secrets. Clone handles and small IDs freely when
  it clarifies ownership.
- Prefer `&str`, slices, and borrowed inputs for pure validation helpers.
- Prefer explicit transactions for multi-row changes.
- Name functions by effect:
  - `verify_*`
  - `promote_*`
  - `enqueue_*`
  - `revoke_*`
  - `purge_*`
  - `record_*`
- Function names that imply safety must actually enforce it. For example,
  `delete_local_original` must check eligibility or be named lower-level, such
  as `request_android_media_delete`.

## Error Handling

- Errors returned to clients use the stable API envelope.
- Internal errors carry enough context for logs without leaking secrets.
- Never log raw passwords, TOTP seeds, recovery codes, session tokens, device
  tokens, share tokens, backup secrets, or object-store credentials.
- Security failures should be intentionally bland to users where detail helps
  attackers.
- Recovery/integrity errors should be specific in admin diagnostics.

## Tests

- Invariants need tests near the code that owns them.
- Every `High` or `Critical` task needs negative tests.
- Pure/value modules should have small table-driven unit tests.
- Cross-boundary behavior needs integration tests:
  - DB plus storage.
  - upload plus job enqueue.
  - auth plus session cookies.
  - backup plus restore.
- Do not mark a task complete because happy-path tests pass if the DoD includes
  a safety or security failure mode.

## Web Style

- React components should be small and feature-named.
- Route components orchestrate; they should not contain raw API calls.
- `src/web/src/api` owns endpoint URLs, DTOs, auth/CSRF handling, and error
  decoding.
- Prefer TanStack Query for server state. Do not mirror server state into a
  second global store.
- Avoid Redux and custom event buses in v1.
- Keep timeline/grid item dimensions stable to prevent layout shifts.
- Do not persist full-size originals in browser storage.
- UI copy should be direct and factual.

## Android Style

- Compose functions render state and emit events; they do not perform network or
  database work.
- ViewModels expose immutable `StateFlow` state.
- Repositories coordinate Room and `MirrorApi`.
- `MirrorApi` owns HTTP details, auth headers, error decoding, upload streaming,
  and retry classification.
- Keep backup, permission, token, and delete flows explicit.
- Local deletion code must document C008 and must not bypass Android's explicit
  media delete flow.
- Prefer restrained Material 3 components and stable dimensions.

## SQL And Migrations

- Migrations are append-only after they are shared.
- Migration names should describe the domain change.
- Prefer constraints and indexes that encode invariants directly.
- Store raw token values nowhere.
- Use `timestamptz` for persisted timestamps.
- Use UUIDv7 for public IDs.
- Avoid database triggers unless they prevent a clear invariant violation that
  application code cannot reliably enforce.

## Tests

- Put project tests under that project's `tests/` directory, for example
  `src/backend/tests/`. Do not put tests inside production source modules such
  as `src/backend/src/*.rs`.
- Tests must target real bugs, invariants, or footguns. Do not test obvious
  facts that the compiler, runtime smoke checks, or framework already cover.
- DB-backed tests are opt-in unless the target provisions its own isolated
  database.

## Documentation Touchpoints

When adding risky code, connect local documentation to global caveats:

- Mention caveat IDs in doc comments for affected functions/types/modules.
- Update `docs/caveats.md` when a new footgun appears.
- Update `docs/tasks.md` completion evidence with tests and caveats.
- Update `docs/v1-architecture.md` only for product, security, or architecture
  decisions, not for local implementation details.

## Inspiration

The style target is close to high-quality Rust infrastructure codebases:

- small explicit modules,
- narrow public APIs,
- strong typed boundaries,
- boring error paths,
- tests around invariants,
- minimal macros and hidden control flow.

Use `rustfmt`, Clippy, compiler errors, and SQLx checks as design feedback, not
as chores to silence.
