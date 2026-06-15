# Mirror V1 Tasks

Task status is tracked here. An LLM or human implementer must update the task
entry when work starts, when caveats are discovered, and when the task is done.

Status markers:

- `[ ]` Not started
- `[~]` In progress
- `[x]` Complete
- `[!]` Blocked

Risk levels: `Low`, `Medium`, `High`, `Critical`.

## Task Template

```markdown
### T000: Short Title

- Status: [ ]
- Milestone:
- Risk:
- Touched subsystems:
- Deliverables:
- Definition of done:
- Required gates:
- Caveats/footguns:
- Completion evidence:
```

## M0: Implementation Control Docs

### T000: Create Implementation Control Docs

- Status: [x]
- Milestone: M0
- Risk: Low
- Touched subsystems: docs
- Deliverables:
  - `docs/implementation-plan.md`
  - `docs/tasks.md`
  - `docs/quality-gates.md`
  - `docs/module-boundaries.md`
  - `docs/llm-workflow.md`
  - `docs/style-guide.md`
  - `docs/caveats.md`
- Definition of done:
  - Docs define task format, risk levels, gates, module boundaries, workflow,
    and caveat tracking.
  - Initial M1-M5 tasks exist.
- Required gates:
  - Readback of created docs.
- Caveats/footguns:
  - These docs are process controls; they do not implement runtime behavior.
- Completion evidence:
  - Commands:
    - `find docs -maxdepth 1 -type f -print | sort` -> passed
    - `sed` readback of all created docs -> passed
  - Files touched:
    - `docs/implementation-plan.md`
    - `docs/tasks.md`
    - `docs/quality-gates.md`
    - `docs/module-boundaries.md`
    - `docs/llm-workflow.md`
    - `docs/style-guide.md`
    - `docs/caveats.md`
  - Tests:
    - Not applicable; docs-only task.

### T001: Duplicate Rust Function Gate

- Status: [x]
- Milestone: M0
- Risk: Low
- Touched subsystems: backend, tests, scripts, docs
- Deliverables:
  - Dependency-free checker for repeated Rust free-function names.
  - Focused checker tests.
  - Backend quality-gate integration.
  - Shared startup and test helpers imported instead of redefined.
- Definition of done:
  - Repeated free-function names fail the backend gate.
  - Required binary `main` functions and `impl`/trait methods remain valid.
  - Existing duplicate helpers are consolidated into named modules.
  - Production, default test, and opt-in database gates pass.
- Required gates:
  - `make check-duplicate-fns`
  - `make gate`
  - `make test-db`
- Caveats/footguns:
  - The checker is intentionally lexical. It detects repeated free-function
    names, not semantically equivalent code, JavaScript/Kotlin duplication, or
    method names scoped by Rust types and traits.
- Completion evidence:
  - Commands:
    - `make check-duplicate-fns` -> passed
    - `make gate` -> passed
    - `make test-db` -> passed
  - Files touched:
    - `scripts/check_duplicate_rust_fns.py`
    - `scripts/tests/test_check_duplicate_rust_fns.py`
    - `Makefile`
    - `src/backend/src/runtime.rs`
    - `src/backend/src/bin/api.rs`
    - `src/backend/src/bin/worker.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/http/auth.rs`
    - `src/backend/src/http/assets.rs`
    - `src/backend/src/http/uploads.rs`
    - `src/backend/tests/support/mod.rs`
    - Backend integration tests using shared storage/count fixtures.
    - `.gitignore`
    - `docs/quality-gates.md`
    - `docs/style-guide.md`
    - `docs/llm-workflow.md`
    - `docs/tasks.md`
  - Tests:
    - Detects repeated free functions across files.
    - Verifies `impl`/trait methods and required binary `main` functions are
      excluded.

## M1: Foundation And Login

### T101: Restructure Repo Into Product Workspaces

- Status: [x]
- Milestone: M1
- Risk: Medium
- Touched subsystems: backend, web, android, infra, docs
- Deliverables:
  - Move the placeholder Rust crate into `src/backend/`.
  - Create empty/scaffolded `src/web/`, `src/android/`, and `infra/` roots.
  - Preserve `docs/` as the canonical planning location.
- Definition of done:
  - Existing Rust placeholder still builds or is intentionally replaced by the
    backend skeleton in T102.
  - No planning docs are lost.
- Required gates:
  - `cargo check` for any Rust workspace that exists.
- Caveats/footguns:
  - Moving files can break relative paths and future commands.
- Completion evidence:
  - Commands:
    - `make gate` -> passed
    - `make gate` -> passed again after moving app roots under `src/`
    - `git status --short` -> workspace contains root workspace files plus
      `src/backend/`, `src/web/`, `src/android/`, `infra/`, and `docs/`
  - Files touched:
    - `Cargo.toml`
    - `Cargo.lock`
    - `src/backend/Cargo.toml`
    - `src/backend/src/lib.rs`
    - `src/backend/src/bin/api.rs`
    - `src/backend/src/config.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/http/health.rs`
    - `src/backend/src/telemetry.rs`
    - `src/web/.gitkeep`
    - `src/android/.gitkeep`
    - `infra/.gitkeep`
    - `Makefile`
    - `docs/tasks.md`
  - Tests:
    - Workspace `cargo check` passed through `make gate`.

### T102: Actix Backend Skeleton

- Status: [x]
- Milestone: M1
- Risk: Medium
- Touched subsystems: backend, infra
- Deliverables:
  - Actix API binary.
  - Config loader.
  - Request ID/tracing middleware.
  - `/health` and `/ready`.
- Definition of done:
  - API starts locally.
  - Health endpoint does not require Postgres.
  - Readiness reports missing dependencies clearly.
- Required gates:
  - `cargo fmt --check`
  - `cargo check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - Relevant backend tests.
- Caveats/footguns:
  - Do not add Actix types to feature logic modules.
  - Do not introduce a DI container or service framework.
- Completion evidence:
  - Commands:
    - `make fmt-check` -> passed
    - `make check` -> passed
    - `make clippy` -> initially found `clone_on_copy`; fixed by removing the
      unnecessary `SocketAddr` clone
    - `make test` -> passed
    - `make gate` -> passed
    - `make gate` -> passed again after moving backend crate to `src/backend/`
    - `cargo run -p mirror-backend --bin api` -> API started with escalation
      because sandbox blocked localhost bind
    - `curl -sS http://127.0.0.1:8080/health` -> `{"status":"ok"}`
    - `curl -sS http://127.0.0.1:8080/ready` ->
      `{"status":"ready","config":"ready"}`
  - Files touched:
    - `src/backend/Cargo.toml`
    - `src/backend/src/lib.rs`
    - `src/backend/src/bin/api.rs`
    - `src/backend/src/config.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/http/health.rs`
    - `src/backend/src/telemetry.rs`
    - `Makefile`
    - `docs/tasks.md`
  - Tests:
    - No behavior-specific tests yet; `cargo test` passed with no tests.
  - Caveats:
    - Dependency fetch required network approval on first `make check`.
    - Localhost bind required escalation for manual endpoint verification.

### T103: Postgres Migrations And Core Schema

- Status: [x]
- Milestone: M1
- Risk: High
- Touched subsystems: backend, database, auth, sessions
- Deliverables:
  - SQLx migration setup.
  - Initial tables for owner account, sessions, device tokens, audit events, and
    rate-limit buckets.
- Definition of done:
  - Migrations apply cleanly to a fresh database.
  - Schema supports hashed session/device tokens and setup-token owner creation.
- Required gates:
  - `cargo check`
  - opt-in backend migration tests against Postgres
  - `cargo sqlx prepare --check` once query macros exist
- Caveats/footguns:
  - Schema changes are hard to unwind once app code depends on them.
  - Token tables must never store raw token values.
- Completion evidence:
  - Commands:
    - `make gate` -> passed
    - `make db-up` -> started local Postgres through `infra/compose.yaml`
    - `make test-db` -> passed after escalation for localhost access
  - Files touched:
    - `src/backend/Cargo.toml`
    - `src/backend/src/lib.rs`
    - `src/backend/src/db.rs`
    - `src/backend/migrations/20260607000100_foundation_auth.up.sql`
    - `src/backend/migrations/20260607000100_foundation_auth.down.sql`
    - `src/backend/tests/migrations.rs`
    - `infra/compose.yaml`
    - `Makefile`
    - `docs/tasks.md`
  - Tests:
    - Default `cargo test` compiles integration tests but does not run DB
      migration tests.
    - Opt-in `make test-db` applies migrations to Postgres.
  - Caveats:
    - Migration tests are opt-in because they require Postgres.
    - Token tables store `token_hash` only, never raw session/device tokens.

### T104: One-Time Setup Token And Owner Creation

- Status: [x]
- Milestone: M1
- Risk: High
- Touched subsystems: backend, auth, security, web, android
- Deliverables:
  - Startup setup token.
  - Owner setup endpoint.
  - Token invalidation after owner creation.
  - Minimal setup UI path for web.
- Definition of done:
  - Unclaimed instance cannot be claimed without token.
  - Setup token is single-use.
  - Owner password is hashed with Argon2id.
- Required gates:
  - backend auth tests
  - CSRF/session tests where applicable
- Caveats/footguns:
  - Do not log passwords or raw session/device tokens.
  - Startup logs containing setup token are sensitive until owner creation.
- Completion evidence:
  - Partial backend slice:
    - Startup generates a one-time setup token only when DB is configured and no
      owner exists.
    - `/setup/owner` creates the owner with Argon2id password hash.
    - Setup token verifier stores only a digest in process state and is consumed
      after owner creation commits.
    - `/ready` reports missing/unreachable Postgres separately from `/health`.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
  - Files touched:
    - `src/backend/Cargo.toml`
    - `src/backend/src/auth/mod.rs`
    - `src/backend/src/auth/password.rs`
    - `src/backend/src/auth/setup_token.rs`
    - `src/backend/src/bin/api.rs`
    - `src/backend/src/config.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/health.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/http/setup.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/src/state.rs`
    - `src/backend/tests/auth_security.rs`
    - `src/backend/tests/setup_owner.rs`
    - `Makefile`
    - `docs/tasks.md`
  - Tests:
    - Default auth security tests cover setup-token rejection/consumption and
      password hash verification/rejection.
    - Opt-in DB test covers password hash persistence and setup-token
      single-use behavior through owner creation.
    - Minimal web setup UI path calls `/setup/owner`.
    - Root `make gate` includes backend and web gates.
  - Web files touched:
    - `src/web/eslint.config.js`
    - `src/web/package.json`
    - `src/web/package-lock.json`
    - `src/web/index.html`
    - `src/web/tsconfig.json`
    - `src/web/vite.config.ts`
    - `src/web/src/api/client.ts`
    - `src/web/src/main.tsx`
    - `src/web/src/styles.css`
    - `src/web/src/test/setup.ts`
    - `src/web/src/ui/App.tsx`
    - `src/web/src/ui/App.test.tsx`
  - Web tests:
    - Default app test verifies first screen is usable owner setup form.

### T105: Web Sessions And Android Device Tokens

- Status: [ ]
- Milestone: M1
- Risk: High
- Touched subsystems: backend, auth, web, android
- Deliverables:
  - DB-backed web sessions with hashed opaque tokens.
  - CSRF protection for cookie-authenticated unsafe requests.
  - Android device-token creation and revocation.
  - Minimal web and Android login.
- Definition of done:
  - Web login/logout/session-list works.
  - Android login stores token in encrypted storage.
  - Revoked tokens stop authorizing requests.
- Required gates:
  - backend auth/session tests
  - web login test
  - Android login/token test
- Caveats/footguns:
  - Cookie `Secure` behavior depends on trusted proxy/HTTPS handling.
  - Android insecure-LAN HTTP must be explicit, never silent.
- Completion evidence:
  - Partial backend slice:
    - Opaque token generation and digest lookup helpers.
    - DB-backed web session create/authenticate/revoke.
    - DB-backed Android device token create/authenticate/revoke.
    - Owner password login service creates a DB-backed web session.
    - `/auth/login` sets an HttpOnly SameSite=Lax session cookie.
    - `/auth/login` sets a readable SameSite=Lax CSRF cookie backed by a
      server-side digest.
    - `/auth/logout` revokes the current session when a cookie is present and
      a matching `x-csrf-token` header is supplied, then expires browser
      cookies.
    - `/sessions` lists active sessions and marks the caller's current session.
    - `/device-tokens` creates an Android token after current-session auth,
      CSRF, and password reauthentication.
    - Minimal web login form calls `/auth/login`.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
  - Files touched:
    - `src/backend/src/auth/device_tokens.rs`
    - `src/backend/src/auth/mod.rs`
    - `src/backend/src/auth/sessions.rs`
    - `src/backend/src/auth/tokens.rs`
    - `src/backend/src/http/auth.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/tests/auth_http.rs`
    - `src/backend/tests/device_token_security.rs`
    - `src/backend/tests/login_security.rs`
    - `src/backend/tests/session_inventory.rs`
    - `src/backend/tests/session_security.rs`
    - `src/backend/tests/token_security.rs`
    - `Makefile`
    - `src/web/eslint.config.js`
    - `src/web/package.json`
    - `src/web/package-lock.json`
    - `src/web/src/api/client.ts`
    - `src/web/src/ui/App.tsx`
    - `src/web/src/ui/App.test.tsx`
    - `docs/tasks.md`
  - Tests:
    - Default tests cover token digest behavior and session cookie flags.
    - Web app test covers setup/login shell rendering.
    - Opt-in DB tests cover wrong password rejection, session authentication,
      session revocation, CSRF-token rejection, device-token authentication,
      device-token revocation, device-token password reauth linkage, and
      session inventory filtering.
  - Still pending for T105:
    - Android login client.

## M2: Upload And Storage Kernel

### T201: OpenDAL Local Storage

- Status: [x]
- Milestone: M2
- Risk: High
- Touched subsystems: backend, storage
- Deliverables:
  - Local filesystem OpenDAL operator.
  - Storage key helpers.
  - Contract tests for put/get/list/delete/promote behavior.
- Definition of done:
  - Storage is outside any writable web root.
  - Generated keys cannot include user path traversal.
- Required gates:
  - backend storage tests
  - `cargo clippy --all-targets --all-features -- -D warnings`
- Caveats/footguns:
  - OpenDAL backends do not all share identical atomic-rename semantics.
- Completion evidence:
  - Commands:
    - `make gate` -> passed
  - Files touched:
    - `src/backend/Cargo.toml`
    - `src/backend/src/lib.rs`
    - `src/backend/src/storage/mod.rs`
    - `src/backend/src/storage/keys.rs`
    - `src/backend/tests/storage_contract.rs`
    - `src/backend/tests/storage_keys.rs`
    - `docs/tasks.md`
  - Tests:
    - Storage key tests reject absolute paths, parent traversal, and
      backslashes.
    - Original key test verifies BLAKE3 content-addressed layout.
    - Local OpenDAL contract test covers write/read/list/promote/delete.
  - Caveats:
    - Promote is copy/delete, not atomic rename; C001 still applies.

### T202: First-Party Chunk Upload Protocol

- Status: [x]
- Milestone: M2
- Risk: High
- Touched subsystems: backend, uploads, storage, web, android
- Deliverables:
  - `POST /uploads`
  - `PUT /uploads/{id}/parts/{part_index}`
  - `GET /uploads/{id}`
  - `POST /uploads/{id}/complete`
  - `DELETE /uploads/{id}`
- Definition of done:
  - Interrupted upload can resume.
  - Completion verifies size/hash and supported media signature.
  - Incomplete staging data is not included in durable backups.
- Required gates:
  - upload integration tests
  - storage contract tests
- Caveats/footguns:
  - Completing upload must be idempotent.
  - A failed verification must not create durable asset state.
- Completion evidence:
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed
  - Files touched:
    - `src/backend/src/bin/api.rs`
    - `src/backend/src/config.rs`
    - `src/backend/src/http/auth.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/http/uploads.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/src/state.rs`
    - `src/backend/src/uploads.rs`
    - `src/backend/migrations/20260607000200_upload_sessions.up.sql`
    - `src/backend/migrations/20260607000200_upload_sessions.down.sql`
    - `src/backend/tests/uploads.rs`
    - `docs/tasks.md`
  - Tests:
    - Upload DB tests cover interrupted/resumable upload via committed part
      indexes.
    - Completion test verifies size, BLAKE3 hash, media signature, and
      idempotent repeated completion.
    - Failed verification test proves session remains `open`, so no durable
      completed state is created.
    - Cancel test proves cancelled uploads reject new parts.
  - Caveats:
    - Completion only verifies staged bytes; T204 owns original promotion,
      asset/source rows, and job enqueue.
    - Staged keys live under `staging/`, so backup code can exclude incomplete
      upload data.

### T203: Android Folder Scan And Upload Worker

- Status: [ ]
- Milestone: M2
- Risk: High
- Touched subsystems: android, backend uploads
- Deliverables:
  - MediaStore folder scan.
  - WorkManager upload job.
  - Wi-Fi default constraint.
  - Upload status UI.
- Definition of done:
  - Android uploads a real photo from the selected folder to the backend.
  - Retry/resume works after process restart.
- Required gates:
  - Android unit/WorkManager tests
  - backend upload tests
- Caveats/footguns:
  - Android partial-media permissions can make backup incomplete.
  - Do not request all-files access.
- Completion evidence:
  - Pending.

### T204: Original Promotion And Job Enqueue

- Status: [x]
- Milestone: M2
- Risk: Critical
- Touched subsystems: backend, storage, uploads, assets, jobs, database
- Deliverables:
  - BLAKE3 verification.
  - Content-addressed original promotion.
  - Asset/source records.
  - Transactional metadata/derivative job enqueue.
- Definition of done:
  - Exact duplicate bytes share one original blob.
  - Failed DB transaction does not leave promoted durable asset state without
    records, or recovery tooling detects/remediates it.
  - Job enqueue rolls back with the asset transaction.
- Required gates:
  - upload/storage/job integration tests
  - data-loss negative tests
- Caveats/footguns:
  - Cross-boundary DB/storage atomicity is impossible; implementation must
    define recovery for orphan staged/promoted objects.
- Completion evidence:
  - Backend implementation:
    - Verified uploads can be promoted into `originals`, `assets`, and
      `asset_sources`.
    - Originals are content-addressed by BLAKE3 and deduplicated across
      repeated exact uploads.
    - Asset jobs are inserted in the same DB transaction as asset/source rows
      with idempotency keys.
    - `detect_original_orphan` detects the DB/storage atomicity caveat from
      C001 for recovery tooling.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
  - Files touched:
    - `src/backend/Cargo.toml`
    - `src/backend/src/assets.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/uploads.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/migrations/20260607000300_assets_jobs.up.sql`
    - `src/backend/migrations/20260607000300_assets_jobs.down.sql`
    - `src/backend/tests/assets_promotion.rs`
    - `src/backend/tests/upload_http.rs`
    - `src/backend/tests/support/mod.rs`
    - `docs/tasks.md`
  - Tests:
    - Asset promotion test proves duplicate bytes share one original while
      keeping distinct asset rows.
    - Repeated promotion of the same upload is idempotent.
    - Unverified uploads cannot create originals, assets, or jobs.
    - Orphan detection finds a promoted object without an `originals` row.
    - HTTP complete-upload route test proves the app path promotes an asset and
      enqueues jobs after session/CSRF checks.
  - Caveats:
    - Storage object writes still cannot be atomically rolled back with
      Postgres; C001 remains the recovery model.

### T205: Durable Job Queue Core

- Status: [x]
- Milestone: M2
- Risk: High
- Touched subsystems: backend, jobs, database
- Deliverables:
  - SQLx job enqueue helper for same-transaction domain changes.
  - `FOR UPDATE SKIP LOCKED` leasing.
  - Worker heartbeat and stale lease reclaim.
  - Retry scheduling and dead-letter state.
- Definition of done:
  - Ready jobs lease in priority/run-after order.
  - A leased job is exclusive until completed, failed, or stale.
  - Stale leases can be reclaimed by another worker.
  - Failed jobs either retry with backoff or move to `dead` at max attempts.
- Required gates:
  - backend job integration tests
  - opt-in Postgres tests
- Caveats/footguns:
  - Job handlers must be idempotent; queue idempotency prevents duplicate rows,
    not duplicate external side effects after a crash.
  - Lease timeout must be longer than expected handler heartbeat interval.
- Completion evidence:
  - Backend implementation:
    - `src/backend/src/jobs.rs` owns enqueue, lease, heartbeat, complete, and
      fail transitions.
    - `assets` now uses the job enqueue helper instead of hand-writing job SQL.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
  - Files touched:
    - `src/backend/src/jobs.rs`
    - `src/backend/src/assets.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/tests/jobs_queue.rs`
    - `src/backend/tests/support/mod.rs`
    - `docs/tasks.md`
  - Tests:
    - Queue tests cover priority/run-after leasing.
    - Queue tests cover exclusive ownership, heartbeat, and completion.
    - Queue tests cover stale lease reclaim.
    - Queue tests cover retry and dead-letter failure paths.

## M3: Media And Timeline

### T301: Metadata And Derivative Worker

- Status: [~]
- Milestone: M3
- Risk: High
- Touched subsystems: backend, media, jobs, storage
- Deliverables:
  - Metadata extraction.
  - AVIF/WebP thumbnails and previews.
  - Video posters.
  - Worker timeout/temp-dir controls.
- Definition of done:
  - Supported fixture media generates expected metadata and preview/poster.
  - Previews strip sensitive metadata while originals preserve it.
- Required gates:
  - media fixture tests
  - worker sandbox tests
- Caveats/footguns:
  - Media parsers are a high-risk attack surface.
  - HEIC/HEIF support depends on host tooling.
- Completion evidence:
  - Backend partial:
    - Added `asset_metadata` and `derivatives` tables.
    - Added `media` module with still-image metadata extraction and WebP
      thumbnail/preview generation handlers.
    - Added `worker` module and `worker` binary for lease-run-complete/fail
      job orchestration.
    - `/assets` exposes generated thumbnail/preview metadata and
      `/assets/{asset_id}/derivatives/{kind}` serves authenticated derivative
      bytes.
    - Still-image processing uses Rust `image` behind an `ImageProcessor`
      trait; video probing/posters remain pending.
    - Derivative storage keys include generator version, kind, format, and
      source original BLAKE3.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
  - Files touched:
    - `src/backend/Cargo.toml`
    - `Cargo.lock`
    - `src/backend/src/lib.rs`
    - `src/backend/src/bin/worker.rs`
    - `src/backend/src/http/assets.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/media.rs`
    - `src/backend/src/worker.rs`
    - `src/backend/src/storage/keys.rs`
    - `src/backend/migrations/20260607000400_media_derivatives.up.sql`
    - `src/backend/migrations/20260607000400_media_derivatives.down.sql`
    - `src/backend/tests/media_worker.rs`
    - `src/backend/tests/worker_runtime.rs`
    - `src/backend/tests/storage_keys.rs`
    - `src/backend/tests/support/mod.rs`
    - `docs/tasks.md`
    - `docs/v1-architecture.md`
  - Tests:
    - Pure processor test verifies a PNG can produce a bounded WebP derivative.
    - DB/storage tests verify metadata and two derivative rows persist.
    - DB/storage tests verify derivative generation is idempotent.
    - Timeline route tests verify derivative metadata and authenticated
      derivative byte serving.
    - Job-handler test rejects invalid payloads before side effects.
    - Worker runtime tests complete queued media jobs and dead-letter a maxed
      failing job.
    - Storage-key test rejects untrusted derivative generator segments.
  - Still pending:
    - Video poster/probe handler.
    - EXIF/GPS extraction.
    - Sandbox/container resource limits for parser attack surface.

### T302: Timeline API And Web/Android Timeline

- Status: [~]
- Milestone: M3
- Risk: Medium
- Touched subsystems: backend, web, android
- Deliverables:
  - Cursor-paginated timeline API.
  - React virtualized timeline.
  - Android dense grid timeline.
- Definition of done:
  - Uploaded asset appears in web and Android timelines with preview.
  - Cursor pagination is stable under additional uploads.
- Required gates:
  - backend timeline tests
  - web Playwright timeline test
  - Android timeline test
- Caveats/footguns:
  - Timeline cursor must not leak internal integer IDs as public API.
  - Completion evidence:
    - Backend partial:
      - `assets::list_assets` returns cursor-paginated owner timeline rows in
        newest-first order.
    - `/assets` returns the authenticated owner's page.
    - Timeline items include generated thumbnail/preview metadata when
      derivatives exist.
      - `/assets/{asset_id}/derivatives/{kind}` serves authenticated derivative
        bytes.
      - Cursor is opaque URL-safe base64 over `(created_at, public_id)` and does
        not expose integer IDs.
    - Web partial:
      - Login success enters a usable timeline view.
      - Timeline renders thumbnail grid items from `/assets` and derivative
        URLs.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
  - Files touched:
    - `src/backend/src/assets.rs`
    - `src/backend/src/http/assets.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/tests/assets_timeline.rs`
    - `src/backend/tests/support/mod.rs`
    - `src/web/src/api/client.ts`
    - `src/web/src/ui/App.tsx`
    - `src/web/src/ui/App.test.tsx`
    - `src/web/src/styles.css`
    - `docs/tasks.md`
  - Tests:
    - Backend tests cover cursor paging without duplicates.
    - Backend tests reject invalid limit/cursor inputs.
    - HTTP route tests cover authenticated timeline response and derivative
      byte serving.
    - Web test covers login-to-timeline rendering with thumbnail URL mapping.
  - Still pending:
    - React timeline pagination/virtualization.
    - Android dense grid timeline.
    - Frontend preview/detail view from T301 derivatives.

## M4: Safety, Sharing, Export, Backup

### T401: Private Shares

- Status: [ ]
- Milestone: M4
- Risk: High
- Touched subsystems: backend, web, security, storage
- Deliverables:
  - Share-token creation, hashing, expiry, revocation.
  - Share pages with privacy headers.
  - Optional original-download policy.
- Definition of done:
  - Revoked/expired links do not work.
  - Share pages omit GPS/full EXIF and people labels by default.
- Required gates:
  - share authorization tests
  - web share-page tests
- Caveats/footguns:
  - Share pages must not include third-party scripts or leak referrers.
- Completion evidence:
  - Pending.

### T402: Trash, Export, Backup, Restore

- Status: [ ]
- Milestone: M4
- Risk: Critical
- Touched subsystems: backend, storage, backups, security
- Deliverables:
  - Trash/restore/purge.
  - Export originals plus manifest.
  - Restic backup orchestration.
  - Restore check and integrity scan.
- Definition of done:
  - Durable-state backup restores into clean Postgres and empty storage.
  - Temp/staging/incomplete uploads/logs/scratch are excluded from backups.
  - Purge is explicit and audited.
- Required gates:
  - backup/restore tests
  - data-loss negative tests
- Caveats/footguns:
  - Backup key is a mounted secret; losing it makes encrypted backups
    unrecoverable.
  - Trash and backup retention can interact in surprising ways.
- Completion evidence:
  - Pending.

### T403: Security And Rate Limits

- Status: [ ]
- Milestone: M4
- Risk: High
- Touched subsystems: backend, security, infra
- Deliverables:
  - Process-local governor limiter.
  - SQLx/Postgres limiter for sensitive flows.
  - Trusted proxy handling.
  - Secret redaction.
- Definition of done:
  - Login/TOTP/recovery/share/upload/export/backup/restore limits work.
  - Proxy headers are trusted only from configured ranges.
  - Secrets are redacted from logs/errors/audit payloads.
- Required gates:
  - security tests
  - rate-limit tests
- Caveats/footguns:
  - Process-local limits are not persistent; only SQLx limits protect sensitive
    flows across restarts.
- Completion evidence:
  - Pending.

## M5: Optional ML

### T501: Model Packs And ML Worker

- Status: [ ]
- Milestone: M5
- Risk: High
- Touched subsystems: backend, ml-worker, models, jobs, storage
- Deliverables:
  - Optional ML worker.
  - Model-pack manifest validation.
  - Model install and reindex state.
- Definition of done:
  - Pinned checksummed model pack can be installed.
  - Invalid checksum/license/self-test blocks activation.
- Required gates:
  - model-pack validation tests
  - worker job tests
- Caveats/footguns:
  - Model downloads are supply-chain sensitive.
  - Embeddings from different revisions cannot be mixed.
- Completion evidence:
  - Pending.

### T502: Semantic Search And People Albums

- Status: [ ]
- Milestone: M5
- Risk: High
- Touched subsystems: backend, ml-worker, search, people, web, android
- Deliverables:
  - SigLIP2 semantic asset embeddings.
  - AuraFace/OpenCV face pipeline.
  - People cluster review.
  - Web/Android search and people surfaces.
- Definition of done:
  - Semantic fixture search ranks expected assets.
  - People name/merge/split/hide flows pass fixture tests.
  - Model revision changes require reindex and do not mix vectors.
- Required gates:
  - ML golden tests
  - backend search/people tests
  - web/Android people/search tests
- Caveats/footguns:
  - Face recognition has privacy and correctness risk; keep it user-enabled.
- Completion evidence:
  - Pending.
