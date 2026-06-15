# Mirror Quality Gates

Quality gates are strict per milestone. A task must pass every gate relevant to
the subsystem it touches. Missing-subsystem gates do not block early milestones,
but once a subsystem exists its relevant gates apply to touched code.

## Diagnostic Policy

- Investigate every compiler/linter warning.
- Do not blindly delete code, prefix variables with `_`, or add suppression
  attributes.
- Suppressions must include a task ID and a specific reason.
- Treat "unused" diagnostics as possible design feedback: stale code, missing
  call site, incomplete invariant, or accidentally dropped behavior.
- Any skipped gate must be recorded in `docs/tasks.md` with reason and follow-up.

## Backend Gates

Run for backend tasks once `src/backend/` exists:

```sh
cargo fmt --check
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
make check-duplicate-fns
```

Additional backend gates when applicable:

```sh
cargo deny check
cargo sqlx prepare --check
make test-db
```

Use `cargo sqlx prepare --check` once SQLx query macros are introduced. Before
that, opt-in migration tests and runtime SQLx integration tests are the gate.
Run DB-backed tests through explicit targets such as `make test-db`; default
`cargo test` should not require Docker/Postgres.

Backend security/data tasks must also include negative tests for the failure
mode they are designed to prevent.

`make check-duplicate-fns` rejects repeated Rust free-function names across
production and test files. Move shared behavior into a precisely named module
and import it. Required binary `main` functions and `impl`/trait methods are
excluded because their names are scoped by Rust rather than shared globally.

## Web Gates

Run for web tasks once `src/web/` exists:

```sh
npm run typecheck
npm run lint
npm test
```

Expected configuration:

- TypeScript `strict`.
- `noUnusedLocals` enabled.
- `noUnusedParameters` enabled unless proven too noisy for React event handlers.
- ESLint with type-checked TypeScript rules.
- Vitest for units/state helpers.
- Playwright for vertical flows.

Web tasks that touch routing/search params must include route-state tests or a
Playwright scenario.

## Android Gates

Run for Android tasks once `src/android/` exists:

```sh
./gradlew lint
./gradlew test
./gradlew assembleDebug
```

Expected configuration:

- Kotlin compiler warnings are investigated.
- Warnings-as-errors is enabled once the initial Android baseline is clean.
- Android Lint runs in CI and fails for relevant errors.
- detekt covers Kotlin static analysis.
- ktlint covers formatting.

Android tasks that touch backup, permissions, token storage, or local deletion
require unit or instrumentation coverage for success and failure paths.

## Infra And Script Gates

Run when infra/scripts exist and are touched:

```sh
docker compose config
shellcheck <script>
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

Docker Compose changes must preserve non-root worker/API containers where
practical and must not publish storage buckets or object directories directly.

## Docs Gates

Docs-only tasks require:

- Link paths remain accurate.
- Architecture/task/caveat docs are mutually consistent.
- Style guide requirements are reflected in task DoD when code is touched.
- No task is marked complete without completion evidence.

## Style Gate

For code tasks, verify:

- Public modules, public items, and invariant-owning types have useful doc
  comments.
- Risky functions mention relevant caveat IDs from `docs/caveats.md`.
- New abstractions are justified by `docs/module-boundaries.md`.
- No new generic `utils`, service-container, repository-trait, or event-bus
  pattern appears without an architecture-doc update.

## Gate Evidence Format

Record in `docs/tasks.md`:

```markdown
- Completion evidence:
  - Commands:
    - `cargo test` -> passed
  - Tests:
    - Added `...`
  - Manual:
    - Verified `...`
  - Caveats:
    - Added/closed `C...`
```
