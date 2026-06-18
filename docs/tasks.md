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
  - `docs/handoff.md`
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

- Status: [x]
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
  - Completed backend/web slice:
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
    - `/auth/device-login` verifies owner password and returns one raw Android
      device token.
    - Owner routes accept either one browser session cookie or one Android
      Bearer token. Mixed credentials are rejected.
    - `/device-tokens/{id}` lets a browser revoke an owner token or a device
      revoke only itself.
    - Minimal web login form calls `/auth/login`.
  - Completed Android slice:
    - Kotlin/Compose app with minimal server/password/device-name login.
    - Ktor `MirrorApi` owns login, Bearer auth, error decoding, and revocation.
    - Android Keystore AES-GCM encrypts the device credential before private
      preference storage; app backup/device transfer excludes credential data.
    - HTTPS is default. HTTP requires explicit opt-in and a private, loopback,
      or link-local address literal.
    - Login state is exposed through immutable `StateFlow`.
    - Gradle wrapper pins Gradle 9.4.1; AGP 9.2.0 uses JDK 17 and SDK 36.
    - detekt, ktlint, compiler warnings-as-errors, Android Lint, unit tests, and
      APK builds are part of root `make gate`.
  - Commands:
    - `make gate` -> passed
    - `make test-db` -> passed against local Postgres
    - `make android-device-test` -> passed on API 36 emulator
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
    - `src/android/settings.gradle.kts`
    - `src/android/build.gradle.kts`
    - `src/android/app/build.gradle.kts`
    - `src/android/app/src/main/AndroidManifest.xml`
    - `src/android/app/src/main/kotlin/app/mirror/vault/auth/`
    - `src/android/app/src/main/kotlin/app/mirror/vault/network/`
    - `src/android/app/src/main/kotlin/app/mirror/vault/ui/`
    - `src/android/tests/`
    - `src/android/tests-instrumentation/`
    - `src/android/config/detekt/detekt.yml`
    - `docs/tasks.md`
  - Tests:
    - Default tests cover token digest behavior and session cookie flags.
    - Web app test covers setup/login shell rendering.
    - Opt-in DB tests cover wrong password rejection, session authentication,
      session revocation, CSRF-token rejection, device-token authentication,
      device-token revocation, device-token password reauth linkage, and
      session inventory filtering.
    - Backend HTTP test covers Android password login, Bearer authorization,
      mixed-credential rejection, self-revocation, and immediate rejection of
      the revoked token.
    - Android unit tests cover endpoint policy, credential persistence, and
      preserving an existing credential when replacement login fails.
    - Android instrumentation test verifies Keystore-encrypted credential
      write/read/clear on API 36.
  - Caveats:
    - C015 records why dynamic private-LAN HTTP requires process-wide manifest
      cleartext support plus strict runtime endpoint validation.

### T106: Complete Web Session Controls

- Status: [x]
- Milestone: M1
- Risk: High
- Touched subsystems: web, auth
- Deliverables:
  - Real CSRF-protected web logout.
  - Active web session inventory.
  - Minimal session management UI.
- Definition of done:
  - Lock/logout revokes the current backend session rather than only changing
    local UI state.
  - Active sessions load after login and failures remain visible/retryable.
  - Web tests cover request credentials, CSRF handling, and state transitions.
- Required gates:
  - Web typecheck, lint, tests, and build.
- Caveats/footguns:
  - CSRF cookie/header behavior must remain aligned with backend auth routes.
- Completion evidence:
  - Web API client sends the CSRF cookie value through `x-csrf-token` for
    logout and loads `/sessions` with credentials.
  - UI exposes photos/session views, real logout, retryable failures, and
    authenticated-state preservation when logout fails.
  - `make gate-web` -> passed: typecheck, lint, six tests, and production build.

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

- Status: [x]
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
  - Android implementation:
    - MediaStore discovers image folders and persists owner selection in Room.
    - Camera is selected once by default when present; explicit deselection is
      preserved.
    - Android 14 partial photo access is detected and shown as degraded.
    - No all-files permission is requested.
    - WorkManager runs immediate and six-hour periodic backup work with
      `UNMETERED` as the default network constraint.
    - Room persists media fingerprints, BLAKE3, client upload key, server
      upload ID, state, errors, and verified asset ID.
    - Interrupted `uploading` rows return to `pending` on the next worker run.
    - Fixed 4 MiB parts resume from server-reported committed indexes.
    - Changed local media invalidates stale hash/session state and cannot be
      marked verified.
    - Backend upload creation accepts an owner-scoped client UUID, making a
      lost create response safe to retry without duplicate sessions.
    - Backup UI exposes permission state, folder selection, Wi-Fi-only setting,
      queue counts, failure state, and manual retry.
  - Current scope:
    - T203 backs up supported still images. Video MediaStore scanning remains
      deferred until backend video upload/processing support is complete.
  - Tests:
    - JVM test proves process-restart resume skips a committed first part.
    - JVM test proves changed media cancels stale server progress.
    - Room instrumentation proves fingerprint changes clear verified progress
      and allocate a new idempotency key.
    - MediaStore instrumentation inserts, discovers, scans, and reads a real
      photo.
    - WorkManager instrumentation covers retry and terminal auth outcomes.
    - Opt-in vertical instrumentation uploads a real MediaStore photo through
      Ktor to the local Actix API and receives a verified asset ID.
    - Backend DB test proves upload-create retry idempotency and rejects reuse
      of a client key with different metadata.
  - Commands:
    - `make test-db` -> passed
    - `make android-device-test` -> passed, 5 tests
    - opt-in Android-to-Actix upload instrumentation -> passed
    - `make gate` -> passed
  - Caveats:
    - C007 remains open for Android permission behavior.
    - C015 remains open for explicit private-LAN HTTP.
    - C016 is mitigated by owner-scoped upload-create idempotency keys.
    - C017 requires explicit Room migrations after schema version 1.

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

### T206: Android Backup Queue Isolation And Batching

- Status: [x]
- Milestone: M2
- Risk: High
- Touched subsystems: android, Room, WorkManager, auth
- Deliverables:
  - Enforce selected and currently available folders at queue claim, status,
    and verification boundaries.
  - Fence durable media state by remote vault generation.
  - Bound each WorkManager execution and append continuation work.
  - Make local disconnect independent from remote revoke availability.
  - Add explicit Room 1-to-2 migration.
- Definition of done:
  - Deselected or unavailable media cannot start or finish an upload.
  - A stale worker from server A cannot mutate server B state.
  - One worker run processes at most four media items.
  - Failed remote revoke still clears local credentials and reports the
    unresolved remote token.
  - Existing Room v1 settings migrate without destructive fallback.
- Required gates:
  - Android JVM tests.
  - Android static analysis, lint, and builds.
  - Opt-in Android instrumentation tests.
- Caveats/footguns:
  - C007: permission changes can make folders temporarily unavailable.
  - C017: every Room entity change requires an explicit migration.
  - C018: v1 remote scope uses normalized server URL, not a stable instance ID.
- Completion evidence:
  - Queue claim/count/verification paths require selected and currently
    available folders.
  - Remote generations fence stale workers; URL changes clear incompatible
    media progress.
  - Worker runs process at most four items and append continuation work.
  - Disconnect clears local credentials before best-effort remote revocation.
  - Room schema 2 includes an explicit 1-to-2 migration and exported schemas.
  - `detekt ktlintCheck test lint assembleDebug assembleDebugAndroidTest`
    -> passed.
  - `connectedDebugAndroidTest` -> passed on API 36.

### T207: Enforce Upload Part Framing

- Status: [x]
- Milestone: M2
- Risk: High
- Touched subsystems: backend, uploads, HTTP, Android contract
- Deliverables:
  - Route-local 4 MiB Actix payload limit.
  - Deterministic part index and length validation.
  - Stable oversized-part error response.
- Definition of done:
  - A full 4 MiB Android part reaches upload storage.
  - Oversized, out-of-range, and incorrectly sized parts create no durable
    part state.
  - A multi-part upload completes through the HTTP route.
- Required gates:
  - Backend default and opt-in database tests.
  - Upload negative tests.
- Caveats/footguns:
  - C019: reverse proxies must allow the protocol part size.
  - Route buffering uses about 4 MiB per concurrent part request.
- Completion evidence:
  - Actix route rejects bodies over 4 MiB with
    `413 upload_part_too_large`.
  - Part index and deterministic part length are validated before storage or
    database writes.
  - Completion requires contiguous framed parts.
  - `video/mp4` uploads are accepted only when the MP4 `ftyp` signature is
    present; DB constraints allow the same media type through promotion.
  - `make gate-backend` -> passed.
  - `make test-db` -> passed against local Postgres after resetting stale test
    schema state.

### T208: Original Storage Integrity Recovery

- Status: [x]
- Milestone: M2
- Risk: Critical
- Touched subsystems: backend, storage, database, operations
- Deliverables:
  - Full original object/row integrity scan.
  - Missing-object and orphan-object report.
  - Explicit orphan remediation with a DB recheck before deletion.
- Definition of done:
  - Scanner reports DB originals missing from storage and storage originals
    missing from DB.
  - Scan is read-only.
  - Deletion requires explicit operator intent and cannot delete an object that
    became DB-referenced after the scan.
- Required gates:
  - Backend default and opt-in database/storage tests.
  - Data-loss negative test for recheck-before-delete.
- Caveats/footguns:
  - C001: no DB transaction can make object deletion atomic.
- Completion evidence:
  - Scanner reports orphan storage objects and missing original objects.
  - Remediation accepts only selected canonical original keys and rechecks
    Postgres immediately before deletion.
  - Maintenance CLI is dry-run by default and requires explicit keys for
    `--apply`.
  - `make gate-backend` -> passed.
  - `make test-db` -> passed against local Postgres after resetting stale test
    schema state.

## M3: Media And Timeline

### T301: Metadata And Derivative Worker

- Status: [x]
- Milestone: M3
- Risk: High
- Touched subsystems: backend, media, jobs, storage
- Deliverables:
  - Metadata extraction.
  - WebP thumbnails and previews.
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
      trait.
    - Still-image source size, decoded dimensions, and decoder allocation are
      bounded before or during processing.
    - Pinned `nom-exif` parses bounded owner-only camera, capture-time, GPS,
      and raw entry data. Missing or malformed optional metadata does not fail
      otherwise valid image processing.
    - Large video originals stream from OpenDAL into private bounded temp
      files instead of one in-memory buffer.
    - Timeout-bounded `ffprobe`/`ffmpeg` handlers extract duration and
      rotation-correct display dimensions, then generate metadata-free WebP
      posters near 10% of playback.
    - Video and still-image derivative generation no longer upscales tiny
      originals; generated preview/thumbnail dimensions are capped by source
      dimensions and requested max edge.
    - HEIC/HEIF upload validation, database constraints, archive export
      extensions, and media-worker derivative generation are supported through
      a timeout-bounded `heif-convert` wrapper that converts to PNG before the
      Rust image pipeline.
    - MP4 upload support now spans app validation, DB constraints, original
      promotion, metadata extraction, and video poster generation.
    - Worker heartbeats prevent live lease reclaim. CPU media work runs on the
      blocking pool; wall timeout records retry and exits the worker process so
      stuck parser threads cannot accumulate.
    - Derivative storage keys include generator version, kind, format, and
      source original BLAKE3.
    - V1 emits WebP only. AVIF is intentionally deferred to avoid format
      negotiation, duplicate derivative storage, and another codec test matrix.
    - Backend container image uses Rust 1.93.0 for builds, ships API, worker,
      and maintenance binaries, installs `ffmpeg` plus `libheif-examples`, and
      runs as UID/GID 10001.
    - Compose defines non-root API and worker services with read-only root
      filesystems, dropped capabilities, `no-new-privileges`, tmpfs scratch
      directories, private storage volume mounts, CPU/memory limits, and PID
      limits.
  - Commands:
    - `make gate-backend` -> passed after MP4/no-upscale integration.
    - `make test-db` -> passed against local Postgres.
    - `docker compose -f infra/compose.yaml config` -> passed.
    - `docker build -f src/backend/Dockerfile -t mirror-backend:dev .` ->
      passed.
    - `docker run --rm --entrypoint /usr/local/bin/mirror-maintenance
      mirror-backend:dev --help` -> passed.
    - `docker run --rm --entrypoint heif-convert mirror-backend:dev --version`
      -> passed with `1.15.1`.
  - Files touched:
    - `Cargo.toml`
    - `infra/compose.yaml`
    - `src/backend/Dockerfile`
    - `src/backend/Cargo.toml`
    - `Cargo.lock`
    - `src/backend/src/lib.rs`
    - `src/backend/src/bin/worker.rs`
    - `src/backend/src/http/assets.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/media.rs`
    - `src/backend/src/video.rs`
    - `src/backend/src/worker.rs`
    - `src/backend/src/storage/keys.rs`
    - `src/backend/migrations/20260607000400_media_derivatives.up.sql`
    - `src/backend/migrations/20260607000400_media_derivatives.down.sql`
    - `src/backend/migrations/20260617000100_video_mp4_uploads.up.sql`
    - `src/backend/migrations/20260617000100_video_mp4_uploads.down.sql`
    - `src/backend/tests/media_worker.rs`
    - `src/backend/tests/uploads.rs`
    - `src/backend/tests/worker_runtime.rs`
    - `src/backend/tests/storage_keys.rs`
    - `src/backend/tests/video_tools.rs`
    - `src/backend/migrations/20260618000600_heic_heif_uploads.up.sql`
    - `src/backend/migrations/20260618000600_heic_heif_uploads.down.sql`
    - `src/backend/tests/support/mod.rs`
    - `docs/tasks.md`
    - `docs/v1-architecture.md`
  - Tests:
    - Pure processor test verifies a PNG can produce a bounded WebP derivative.
    - Processor test rejects image dimensions above the decoder bound.
    - Hand-built JPEG fixture verifies camera, capture time, GPS extraction,
      and that generated WebP derivatives do not preserve owner metadata.
    - External-tool tests verify real MP4 probing/poster generation, command
      timeout kill, rotation-correct display dimensions, and no upscaling for
      extensionless staged video inputs.
    - Upload tests verify MP4 `ftyp` signature acceptance.
    - Processor tests verify HEIC conversion reports a missing converter
      cleanly, and uses real host `heif-enc`/`heif-convert` when both are
      available.
    - Upload tests verify HEIC `ftyp` signature acceptance.
    - Storage test verifies oversized streamed staging removes its partial
      file.
    - Opt-in worker tests cover heartbeat lease retention and timeout retry.
    - Opt-in DB/storage test covers streamed MP4 metadata and two persisted
      poster derivatives.
    - DB/storage tests verify metadata and two derivative rows persist.
    - DB/storage tests verify derivative generation is idempotent.
    - Timeline route tests verify derivative metadata and authenticated
      derivative byte serving.
    - Job-handler test rejects invalid payloads before side effects.
    - Worker runtime tests complete queued media jobs and dead-letter a maxed
      failing job.
    - Storage-key test rejects untrusted derivative generator segments.
### T302: Timeline API And Web/Android Timeline

- Status: [x]
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
    - Added owner favorite mutations: `POST /assets/{asset_id}/favorite` and
      `DELETE /assets/{asset_id}/favorite`. Mutations are idempotent for active
      assets and timeline rows expose `favorite_at`.
    - Added `search` module and authenticated `/search` route for exact
      filename metadata search over active owner assets. Search results reuse
      timeline item shape so web/Android can share rendering.
    - Filename search trims/lowercases bounded queries, escapes SQL wildcard
      characters, excludes trashed assets, and returns newest-first rows.
    - Cursor is opaque URL-safe base64 over `(created_at, public_id)` and does
      not expose integer IDs.
  - Web:
    - Cursor pages append through React Query infinite queries without
      replacing existing assets.
    - TanStack Virtual bounds mounted timeline rows while retaining a manual
      load-more fallback and scroll-triggered fetching.
    - Dense responsive grid includes video markers and opens an immersive
      keyboard-navigable preview viewer.
    - Vite development proxy includes `/assets`.
    - Web tests live under `src/web/tests`, outside implementation source.
    - Vite 8 resolves the reported esbuild advisory; `npm audit` reports zero
      vulnerabilities.
  - Android:
    - `KtorMirrorApi` loads cursor-paginated `/assets` pages with Bearer auth.
    - Compose timeline opens on the connected screen as a dense adaptive grid
      with authenticated Coil thumbnail/preview loading, video markers,
      automatic near-end page loading, manual load-more fallback, and a
      full-screen preview dialog.
    - Invalid stored endpoint data cannot crash Compose image rendering; bad
      derivative URLs fall back to placeholders.
    - Stale page responses from a previous credential cannot overwrite the
      current timeline state.
  - Commands:
    - `make gate-backend` -> passed
    - `make gate-web` -> passed: typecheck, lint, seven Vitest tests,
      Playwright, and production build
    - `make gate-android` -> passed: detekt, ktlint, unit tests, Android lint,
      and debug APK build
    - `make test-db` -> passed against local Postgres.
  - Files touched:
    - `src/backend/src/assets.rs`
    - `src/backend/src/http/assets.rs`
    - `src/backend/src/search.rs`
    - `src/backend/src/http/search.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/tests/assets_timeline.rs`
    - `src/backend/tests/search.rs`
    - `src/backend/tests/support/mod.rs`
    - `src/web/src/api/client.ts`
    - `src/web/src/ui/App.tsx`
    - `src/web/src/styles.css`
    - `src/web/tests/App.test.tsx`
    - `src/web/tests/timeline.e2e.ts`
    - `src/web/playwright.config.ts`
    - `src/web/package.json`
    - `src/web/package-lock.json`
    - `src/android/app/build.gradle.kts`
    - `src/android/app/src/main/kotlin/app/mirror/vault/MainActivity.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/MirrorApplication.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/network/MirrorApi.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/network/TimelineApi.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/timeline/TimelineRepository.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/timeline/TimelineViewModel.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/ui/MirrorApp.kt`
    - `src/android/app/src/main/kotlin/app/mirror/vault/ui/TimelineContent.kt`
    - `src/android/tests/app/mirror/vault/timeline/TimelineViewModelTest.kt`
    - `docs/tasks.md`
  - Tests:
    - Backend tests cover cursor paging without duplicates.
    - Backend tests reject invalid limit/cursor inputs.
    - HTTP route tests cover authenticated timeline response and derivative
      byte serving.
    - Backend tests cover favorite/unfavorite idempotency and timeline
      `favorite_at` visibility through authenticated routes.
    - Backend search tests cover active filename matches, trash exclusion,
      wildcard escaping, invalid input, and authenticated `/search` response.
    - Web test covers login-to-timeline rendering with thumbnail URL mapping.
    - Vitest covers cursor-page append and preview open/close behavior.
    - Playwright verifies 120-item DOM virtualization, second-page loading,
      preview rendering, and desktop/mobile layouts in system Chromium.
    - Android unit test covers page append, stale credential response
      isolation, and render-safe invalid derivative URL fallback.

## M4: Safety, Sharing, Export, Backup

### T401: Private Shares

- Status: [~]
- Milestone: M4
- Risk: High
- Touched subsystems: backend, web, security, storage
- Deliverables:
  - Backend share-token creation, hashing, expiry, revocation.
  - Public backend share metadata and derivative routes with privacy headers.
  - Optional original-download policy stored, with original-byte route deferred.
  - Web share page.
- Definition of done:
  - Revoked/expired links do not work.
  - Share pages omit GPS/full EXIF and people labels by default.
- Required gates:
  - backend share authorization/privacy tests
  - web share-page tests
- Caveats/footguns:
  - Share pages must not include third-party scripts or leak referrers.
- Completion evidence:
  - Backend implemented:
    - `asset_shares` migration with token hash, expiry, revocation, owner and
      asset references.
    - `shares` module for create/load/revoke and derivative lookup.
    - Owner routes: `POST /assets/{asset_id}/shares`, `DELETE /shares/{share_id}`.
    - Public routes: `GET /shares/{token}`,
      `GET /shares/{token}/derivatives/{kind}`.
    - Public responses set `Cache-Control: private, no-store`,
      `Referrer-Policy: no-referrer`, `X-Robots-Tag: noindex`, and
      `X-Content-Type-Options: nosniff`.
    - Shared public derivative policy avoids owner/share route drift.
    - Share creation is owner-rate-limited before token minting, so rejected
      requests do not leave usable unreturned raw tokens.
  - Commands:
    - `make gate-backend` -> passed.
    - `make test-db` -> passed.
  - Tests:
    - `src/backend/tests/shares.rs` verifies raw tokens are not stored,
      metadata omits owner-private fields, derivative bytes are served only via
      valid share token, privacy headers are present, and revocation disables
      the link.
    - Share creation rate-limit test verifies the first rejected request returns
      `429` and does not insert an extra share row.
  - Still pending:
    - Web share page and web route tests.
    - Original-byte download route/auditing/range behavior if enabled later.

### T402: Trash, Export, Backup, Restore

- Status: [~]
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
  - Backend partial:
    - Added `assets.trashed_at` migration plus active/trash indexes.
    - Added `DELETE /assets/{asset_id}` to move an owner asset to trash.
    - Added `POST /assets/{asset_id}/restore` to restore a trashed owner asset.
    - Added cursor-paginated `GET /trash/assets` for newest-trashed-first
      owner trash listing.
    - Active timeline and owner derivative reads exclude trashed assets.
    - Share creation and existing public share reads exclude trashed assets.
    - Trash is reversible and does not delete original or derivative objects.
    - Added explicit `DELETE /assets/{asset_id}/purge` for already-trashed
      assets only.
    - Purge deletes the asset row, cascades metadata/derivatives/shares, deletes
      the original database row only when no other asset references it, and
      writes an `asset.purge` audit row in the same transaction.
    - Purge does not delete content-addressed original bytes directly; any
      unreferenced object cleanup remains explicit integrity remediation.
    - Added authenticated `GET /exports/originals/manifest` for active-original
      export metadata: asset ID, content hash, storage key, media type, byte
      size, source filename, and creation timestamp.
    - Added authenticated `GET /exports/originals/{asset_id}` to stream one
      active original by manifest asset ID without materializing the whole
      object in memory.
    - Added authenticated `GET /exports/originals/archive.tar` to stream a
      V1 tar export containing `manifest.json` plus active original bytes,
      excluding trashed assets and avoiding whole-archive materialization.
    - Archive tar support is intentionally limited to regular files with
      simple generated paths; use a maintained tar crate instead of extending
      this manual subset if V1 scope grows to long paths, directories,
      compression, metadata preservation, symlinks, or broader archive
      compatibility.
    - Added durable storage backup manifest selection that includes only
      content-addressed originals, generated derivatives, and model-pack files,
      excluding staging, temp, logs, and scratch by construction.
    - Added `pg_dump -Fc` command planning with database host, port, user,
      password, database name, and optional sslmode supplied through libpq
      environment variables instead of a database URL in argv.
    - Added restic backup command planning for explicit durable originals,
      derivatives, model-pack files, plus a DB dump path, with
      repository/password supplied through environment variables rather than
      argv.
    - Added `maintenance --backup-plan PG_DUMP_PATH` dry-run output for the
      pg_dump/restic programs, args, and required secret environment variables.
    - Added `maintenance --run-backup PG_DUMP_PATH --repository-hint HINT` to
      create a `backup_runs` row, run pg_dump, execute restic, persist the
      parsed snapshot ID on success, and persist failure status on pg_dump or
      restic command failure.
    - Added fixed v1 restic retention planning and execution:
      `forget --prune`, keep last 3, hourly 24, daily 30, weekly 12, monthly
      12, yearly 3, grouped by host and paths with a 30 minute lock retry.
    - Added `maintenance --retention-plan` and `maintenance --run-retention`.
    - Added systemd service/timer examples for daily one-shot backups followed
      by retention pruning. Backups are scheduled outside the app worker so OS
      supervision, persistent timers, logs, and mounted secrets handle the
      durable orchestration boundary.
    - Added `scripts/install_systemd_backup.sh` for host installs: builds and
      installs `/usr/local/bin/mirror-maintenance`, installs the systemd
      service/timer, creates `/etc/mirror/maintenance.env` from an example when
      missing, reloads systemd, and can enable the timer.
    - Added restic restore and `pg_restore` command planning for restore drills,
      keeping repository/password and destination DB URL out of argv. Restore
      supplies only the non-secret database name to `pg_restore --dbname` and
      keeps host/user/password in environment variables.
    - Added `maintenance --restore-plan SNAPSHOT_ID RESTORE_TARGET
      PG_DUMP_PATH` dry-run output for the restic restore and pg_restore
      programs, args, and required secret environment variables.
    - Added `maintenance --run-restore SNAPSHOT_ID RESTORE_TARGET
      PG_DUMP_PATH` to execute restic restore followed by `pg_restore` into
      `MIRROR_DATABASE_URL`, with repository/password and database URL kept out
      of argv/output.
    - Added `maintenance --restore-check BACKUP_RUN_ID` to scan the currently
      configured restored Postgres/storage pair, record restore-check status,
      and fail the process when originals are missing or orphaned.
    - Backup success/failure and restore-check success/failure status
      transitions insert system audit events in the same transaction as the
      `backup_runs` update.
  - Commands:
    - `make gate-backend` -> passed.
    - `make test-db` -> passed.
    - `scripts/backup_restore_drill.sh` against Docker Postgres and real
      restic/pg_dump/pg_restore restored two assets, two originals, one
      derivative, and one model-pack file into a clean database and empty
      storage root, ran real restic retention, excluded staging/tmp/logs/scratch
      files from restore, and `maintenance --restore-check` marked the restored
      run `restore_check_succeeded`.
    - `make backup-restore-drill` -> passed.
    - `scripts/install_systemd_backup.sh --dry-run` -> passed.
  - Tests:
    - Asset timeline integration test verifies trash hides an asset from
      timeline and derivative routes, then restore returns it to the timeline.
    - Trash listing integration test verifies cursor paging, no duplicates, and
      no active asset leakage into trash results.
    - Share integration test verifies an existing share returns `404` after its
      asset is trashed.
    - Purge integration test verifies active assets cannot be purged, trashed
      assets can be purged, share rows cascade away, unreferenced original DB
      rows are removed, and an audit row records permanent removal.
    - Export manifest integration test verifies active originals are listed with
      durable storage metadata and trashed assets are excluded.
    - Export byte integration test verifies active original bytes stream with
      content type/length headers and trashed originals return `404`.
    - Export archive integration test verifies the streamed tar contains the
      JSON manifest and active original bytes while excluding trashed assets.
    - Backup manifest test verifies durable originals/derivatives/model-pack
      files are selected and staged upload objects are excluded.
    - Postgres dump plan tests verify custom-format dump args, secret-free
      argv/debug output, and parent-relative path rejection.
    - Restic plan tests verify durable paths are explicit, originals,
      derivatives, and model-pack files are included, broad storage-root backup
      is not used, secret values are not represented in argv, and parent
      relative paths are rejected.
    - Retention plan tests verify the fixed v1 `forget --prune` policy and
      secret-free argv/debug output.
    - Maintenance CLI tests verify retention-plan output without database
      access and fake-restic `--run-retention` execution without database
      access.
    - Restore plan tests verify snapshot ID validation, secret-free restic and
      pg_restore argv/debug output, and clean restore args.
    - Maintenance CLI test verifies backup-plan output works without database
      access and reports pg_dump/restic inputs/env requirements.
    - Maintenance CLI test verifies restore-plan output works without database
      access and reports restic/pg_restore inputs/env requirements.
    - Maintenance CLI DB test uses fake restic/pg_restore executables to verify
      restore execution order, required secret environment, destination database
      environment, and secret-free output.
    - Maintenance CLI DB test uses fake pg_dump/restic executables to verify
      backup execution creates the dump with split libpq environment variables,
      records a succeeded run, parsed snapshot ID, redacted repository hint, and
      secret-free manifest/output.
    - Maintenance CLI restore-check test verifies a missing restored original
      object marks the backup run `restore_check_failed` with mismatch counts.
    - Backup run DB test verifies backup and restore-check status transitions
      create secret-free audit events.

### T403: Security And Rate Limits

- Status: [~]
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
  - Backend partial:
    - Added `rate_limit` module backed by `rate_limit_buckets`.
    - Owner password login attempts are keyed by request peer IP and stored as
      keyed BLAKE3 hashes; raw IPs and credentials are not stored.
    - `/auth/login` and `/auth/device-login` reject blocked buckets before
      password verification, record failed password attempts, and clear the
      bucket on successful authentication.
    - `MIRROR_RATE_LIMIT_SECRET` supplies stable key material. If absent, local
      dev uses a process-random fallback, which intentionally does not provide
      restart-stable buckets.
    - API returns stable `429 rate_limited` responses for blocked login flows.
    - `Config` debug output redacts `MIRROR_DATABASE_URL` and
      `MIRROR_RATE_LIMIT_SECRET` material.
    - `MIRROR_TRUSTED_PROXIES` accepts comma-separated CIDRs. `X-Forwarded-For`
      is trusted only when the immediate peer IP is inside those ranges.
    - Forwarded client IP resolution walks the header chain right-to-left and
      returns the nearest untrusted address, falling back to the socket peer.
    - Owner password login rate limits use the trusted-proxy-aware client IP;
      spoofed forwarded headers from untrusted peers do not bypass buckets.
    - Share creation uses the same Postgres limiter with an owner-scoped key,
      limiting public-link minting before tokens are generated.
    - Upload creation, upload part writes, and upload completion use
      owner-scoped Postgres quota buckets before session creation, body
      staging, or hash verification/promotion.
    - Original export manifest and original byte routes use owner-scoped
      Postgres quota buckets before producing manifests or streaming bytes.
    - `OpaqueToken` and `TokenHash` debug output is redacted so accidental
      diagnostic formatting cannot expose session, CSRF, device, or share
      tokens.
  - Commands:
    - `make gate-backend` -> passed.
    - `make test-db` -> passed against local Postgres.
  - Files touched:
    - `src/backend/src/config.rs`
    - `src/backend/src/http/auth.rs`
    - `src/backend/src/http/client_ip.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/http/mod.rs`
    - `src/backend/src/http/exports.rs`
    - `src/backend/src/http/shares.rs`
    - `src/backend/src/http/uploads.rs`
    - `src/backend/src/auth/tokens.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/src/rate_limit.rs`
    - `src/backend/tests/client_ip.rs`
    - `src/backend/tests/config_security.rs`
    - `src/backend/tests/device_token_security.rs`
    - `src/backend/tests/exports.rs`
    - `src/backend/tests/support/mod.rs`
    - `src/backend/tests/token_security.rs`
    - `src/backend/tests/upload_http.rs`
    - `docs/tasks.md`
  - Tests:
    - HTTP device-login test proves repeated failed owner-password attempts
      return `429` and block a later correct password without issuing a device
      token.
    - Config security test proves secret-bearing config values are redacted
      from debug output.
    - Client-IP tests prove forwarded headers are ignored from untrusted peers
      and honored from configured trusted proxy CIDRs.
    - Device-login rate-limit test changes spoofed forwarded IPs across
      attempts and still receives `429`, proving untrusted forwarded headers do
      not bypass the limiter.
    - Share route test verifies repeated share creation returns `429` before
      creating another `asset_shares` row.
    - Export manifest route test verifies repeated manifest reads return `429`
      and store only hashed owner-scoped limiter keys.
    - Upload-create route test verifies a persisted limiter bucket returns
      `429` before inserting an upload session.
    - Token security test verifies debug output redacts raw tokens and token
      hashes.
  - Still pending:
    - Process-local limiter for cheap prefiltering if still needed.
    - Rate limits for future upload/export/backup/restore/recovery flows as
      those routes land.

## M5: Optional ML

### T501: Model Packs And ML Worker

- Status: [~]
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
  - Model-pack schema records runtime, task kind, revision, embedding
    dimension, distance metric, license, file checksums, and self-test status.
  - Backend feature code depends on task-level embedding outputs, not ONNX
    sessions or runtime-specific handles.
- Required gates:
  - model-pack validation tests
  - worker job tests
- Caveats/footguns:
  - Model downloads are supply-chain sensitive.
  - Embeddings from different revisions cannot be mixed.
- Completion evidence:
  - Planned decisions:
    - V1 starts with ONNX Runtime model packs, but `runtime` remains explicit
      so a future runtime can be added only for a concrete model need.
    - Model inference boundaries are task-level (`embed_image`, `embed_text`,
      later `detect_faces`/`embed_face`), not generic model-executor
      abstractions.
  - Backend partial:
    - Added `model_packs` and `model_pack_files` tables for task kind,
      runtime, model key/revision, license, embedding dimension, distance
      metric, full manifest JSON, file checksums/sizes, self-test status, and
      active/install state.
    - Added a partial unique index enforcing at most one active model pack per
      task kind.
    - Added `models` module with pure manifest validation plus install,
      self-test status recording, and activation functions.
    - Manifest validation rejects missing license metadata, unsupported runtime
      or distance metric, unsafe relative paths, duplicate file paths, invalid
      SHA-256 digests, non-positive file sizes, invalid embedding dimensions,
      and missing golden self-tests.
    - Manifest validation now requires explicit ONNX runtime wiring:
      image/text model paths, tokenizer path, image/text tensor names, and image
      preprocessing dimensions/channel order/tensor layout/mean/std. Runtime
      file paths must point at declared checksummed model-pack files.
    - Activation is blocked until self-tests pass; activating a new pack
      deactivates the previous active pack for the same task kind.
    - Added `model_reindex_runs` table for durable model-pack reindex
      progress: queued/running/succeeded/failed/canceled status, selected asset
      count, queued asset count, processed/failed counters, and error message.
    - Added `embed_asset` job kind for ML embedding work.
    - Added worker job-kind leasing so a media-only worker does not claim ML
      jobs when no ML runtime is configured.
    - Added `start_model_reindex` to require a self-tested model pack, select
      active non-trashed assets, create a reindex run, and enqueue one
      idempotent `embed_asset` job per selected asset.
    - Added `model_reindex_assets` for per-asset queued/done/failed state and
      idempotent terminal result recording.
    - Added `record_reindex_asset_result` to update processed/failed counters,
      transition a run to running/succeeded/failed, preserve terminal asset
      errors, and ignore duplicate terminal reports.
    - Added `model-packs/{model_pack_id}/...` storage keys for installed model
      files with path traversal/backslash/absolute-path rejection.
    - Added `install_model_pack_files` to copy files from an operator-provided
      local directory into durable storage only after manifest path, size, and
      SHA-256 verification.
    - Durable backup selection now includes installed model-pack files alongside
      originals and derivatives.
    - Added task-level embedding output validation for runtime outputs:
      expected dimension, finite floats, and non-zero cosine vectors.
    - Added `ml` module with task-level `ImageTextEmbedder` boundary and
      `embed_asset` job handling: load active image original, run embedder,
      validate vector, upsert semantic index row, and mark reindex progress
      succeeded.
    - Added `MlRuntime` wrapper so synchronous image/text embedding runs on
      Tokio's blocking pool behind a concurrency limit instead of blocking async
      workers.
    - Added explicit `MIRROR_ML_DEVICE` policy parsing with
      GPU-with-CPU-fallback as the default operator intent; real runtime device
      selection remains pending with the runtime embedder.
    - Added configurable bounded object reads for embedding inputs
      (`MIRROR_ML_MAX_IMAGE_BYTES`, default 25 MiB) and strict v1 still-image
      media-type handling: JPEG/PNG only until GIF/WebP animation semantics are
      explicit.
    - Embed job payloads are cross-checked against reindex run/asset/model
      rows, and final semantic-index plus reindex-progress writes happen in one
      transaction.
    - Queue failure reporting now returns a `JobFailureOutcome`; worker
      dead-letter handling records retry-exhausted `embed_asset` jobs as failed
      reindex assets.
    - Added owner-authenticated model-pack admin routes to list installed packs,
      install manifest metadata, record self-test status, activate a passed
      pack, start a reindex run, and list recent reindex runs.
    - Added runtime-backed golden self-test execution: read installed fixture
      bytes from durable model-pack storage, run the configured image/text
      runtime, validate embedding shape/floats, compare SHA-256 of the f32
      output, and record passed/failed status.
  - Commands:
    - `make gate-backend` -> passed.
    - `make test-db` -> passed.
  - Tests:
    - Pure model-pack validation test covers missing license, duplicate file
      path, parent-relative path, invalid checksum, missing self-tests, missing
      ONNX runtime files, and invalid preprocessing values.
    - DB model-pack test verifies install starts pending, failed self-test
      blocks activation, passed self-test allows activation, and only one pack
      per kind remains active.
    - DB reindex test verifies self-test gating, trashed asset exclusion, run
      counters, and `embed_asset` job payload/idempotency creation.
    - DB reindex progress test verifies successful and failed terminal asset
      results update run counters/status without double-counting duplicate
      reports.
    - Worker runtime DB test verifies an `embed_asset` job writes a semantic
      embedding and marks the reindex run succeeded.
    - Worker runtime DB test verifies a retry-exhausted `embed_asset` job
      records failed reindex progress when it dead-letters.
    - Model-pack HTTP DB test verifies install, self-test gating, activation,
      reindex start, reindex-run listing, and list response shape through
      authenticated routes.
    - Model-pack HTTP DB test verifies `/model-packs/{id}/self-test/run`
      records a passed status when the configured runtime output matches the
      golden fixture digest.
    - Model-pack file install test verifies checksum/size enforcement and
      durable `model-packs/` storage namespace writes.
    - Embedding output validation test rejects wrong dimensions, NaN, and
      zero-norm cosine vectors.
    - Storage-key test verifies model-pack file keys accept only nested
      relative pack paths.
    - Backup manifest test verifies installed model-pack files are included in
      durable backup selection.
  - Files touched:
    - `src/backend/migrations/20260618000100_model_packs.up.sql`
    - `src/backend/migrations/20260618000100_model_packs.down.sql`
    - `src/backend/migrations/20260618000200_model_reindex_runs.up.sql`
    - `src/backend/migrations/20260618000200_model_reindex_runs.down.sql`
    - `src/backend/migrations/20260618000300_model_reindex_assets.up.sql`
    - `src/backend/migrations/20260618000300_model_reindex_assets.down.sql`
    - `src/backend/src/models.rs`
    - `src/backend/src/http/model_packs.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/ml.rs`
    - `src/backend/src/jobs.rs`
    - `src/backend/src/worker.rs`
    - `src/backend/src/bin/worker.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/tests/model_packs.rs`
    - `src/backend/tests/worker_runtime.rs`
    - `src/backend/tests/support/mod.rs`
  - Still pending:
    - Real ONNX runtime embedder.
    - Real model-pack install UI.

### T502: Semantic Search And People Albums

- Status: [~]
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
  - Semantic search uses a Mirror-specific index boundary over asset
    embeddings, not a generic vector database abstraction.
- Required gates:
  - ML golden tests
  - backend search/people tests
  - web/Android people/search tests
- Caveats/footguns:
  - Face recognition has privacy and correctness risk; keep it user-enabled.
- Completion evidence:
  - Planned decisions:
    - Semantic search uses a vision-language model to create image/text
      embeddings; pgvector stores/searches numeric vectors only.
    - V1 vector backend is Postgres/pgvector for simpler joins, backups, and
      owner/trash/privacy filtering. A separate vector DB is deferred until
      measured scale/latency requires it.
  - Backend partial:
    - Compose now uses `pgvector/pgvector:pg16` for the development Postgres
      service.
    - Added `asset_embeddings` migration with `CREATE EXTENSION IF NOT EXISTS
      vector`, asset/model-pack scoped vector storage, dimension metadata, and
      cascade cleanup.
    - Added `semantic_index` module with Mirror-specific
      `upsert_asset_embedding`, `delete_asset_embeddings`, and
      `semantic_search` functions.
    - Added semantic-search hardening migration that denormalizes owner,
      public asset ID, created time, and trash state onto `asset_embeddings`,
      keeps them synchronized from `assets`, and adds the active owner/model
      btree filter index.
    - Semantic search validates model-pack task kind and embedding dimension,
      uses the model-pack distance metric (`cosine`, `dot`, or `l2`), filters
      out trashed assets, and returns public asset IDs ordered by a normalized
      score where higher is better.
    - Dot-product search converts pgvector's negative inner-product rank into a
      positive similarity score before returning hits.
    - Replaced manual vector literal construction with `pgvector::Vector`
      binding for sqlx queries.
    - ML `embed_asset` uses already-loaded model metadata when upserting the
      vector, avoiding a second model-pack metadata query per asset.
    - Added text-query semantic search boundary that embeds the owner query with
      the active self-tested semantic model pack and searches the existing
      asset embedding index.
    - Added `/search?mode=semantic` for semantic text queries when an
      image/text runtime is configured in app data; filename search remains the
      default `/search` mode.
    - Active semantic model-pack selection is deterministic even under data
      corruption with more than one active row.
    - ML worker test path now proves image embeddings produced by the task-level
      embedder are persisted into pgvector and visible through semantic search.
    - Documented ANN decision: exact search for v1, HNSW before IVFFlat when
      scale requires ANN, operator-class matching by metric, and filtered-ANN
      recall tests before enabling ANN.
    - Documented future ANN footguns: plain `vector` storage may require
      expression/partial indexes per model dimension, and bulk reindex should
      avoid per-row metadata fetches or row-by-row HNSW maintenance if measured
      slow.
  - Commands:
    - `docker compose -f infra/compose.yaml config` -> passed.
    - `make gate-backend` -> passed.
    - `make test-db` -> passed.
  - Tests:
    - Semantic index DB test verifies pgvector ranking returns active,
      non-trashed owner assets in one model space.
    - Semantic index DB test verifies wrong-dimension embeddings are rejected
      and asset embedding deletion removes search hits.
    - Semantic index DB test verifies dot-product results return positive
      similarity scores instead of pgvector negative inner-product ranks.
    - Semantic index DB test verifies the active model/owner filter index
      exists for the exact pgvector search path.
    - Semantic index DB test verifies indexed assets disappear from semantic
      results after trash and reappear after restore through the denormalized
      projection trigger.
    - Semantic index DB test verifies wrong model kinds are rejected.
    - Search route DB test verifies `/search?mode=semantic` uses the active
      model pack and returns the ranked asset through the normal asset response
      shape.
    - Worker runtime DB test verifies `embed_asset` creates a pgvector-backed
      search hit for the promoted asset and completes the reindex run.
    - Worker runtime DB test verifies text search uses the active model pack to
      embed a query and returns the matching promoted asset.
  - Files touched:
    - `infra/compose.yaml`
    - `src/backend/migrations/20260618000400_asset_embeddings.up.sql`
    - `src/backend/migrations/20260618000400_asset_embeddings.down.sql`
    - `src/backend/migrations/20260618000500_semantic_search_hardening.up.sql`
    - `src/backend/migrations/20260618000500_semantic_search_hardening.down.sql`
    - `src/backend/src/semantic_index.rs`
    - `src/backend/src/ml.rs`
    - `src/backend/src/search.rs`
    - `src/backend/src/http/search.rs`
    - `src/backend/src/http/error.rs`
    - `src/backend/src/config.rs`
    - `src/backend/src/worker.rs`
    - `src/backend/src/lib.rs`
    - `src/backend/tests/semantic_index.rs`
    - `src/backend/tests/search.rs`
    - `src/backend/tests/worker_runtime.rs`
    - `src/backend/tests/support/mod.rs`
    - `docs/references/pgvector-ann.md`
  - Still pending:
    - Real model runtime that produces image/text embeddings.
    - Web/Android search UI.
    - Face detection/embedding, people clustering, and people review flows.

## Backend Handoff Queue

These are the next highest-value backend tasks for another agent. They are
scoped to backend and database work unless explicitly noted.

### T503: Real ONNX Image/Text Embedder

- Status: [ ]
- Milestone: M5
- Risk: High
- Touched subsystems: backend, ml-worker, model-packs, semantic-search
- Deliverables:
  - Production `ImageTextEmbedder` implementation backed by ONNX Runtime.
  - Tokenizer loading from the active model pack.
  - Image preprocessing from `image_preprocess` manifest config.
  - Explicit device selection honoring `MIRROR_ML_DEVICE` with GPU-preferred
    fallback behavior.
  - Runtime golden self-tests using installed model-pack files, not fake test
    embedders.
- Definition of done:
  - A valid installed semantic model pack can embed image bytes and text query
    strings through the existing task-level `ImageTextEmbedder` boundary.
  - Output vectors pass existing dimension/finite/non-zero validation.
  - Blocking inference never runs on the async reactor thread.
  - Runtime errors are typed and map to safe API/worker failures without
    leaking local paths or model internals.
- Required gates:
  - `make gate-backend`
  - `make test-db`
  - New model-runtime tests gated so they skip cleanly when large model
    fixtures are absent.
- Caveats/footguns:
  - Do not add a generic model-executor abstraction; keep the boundary at
    `embed_image` and `embed_text`.
  - Do not assume every ONNX vision-language model uses the same tensor names,
    layout, normalization, or tokenizer files.
  - Device fallback must be explicit in logs because silent CPU fallback can
    look like a performance bug.
- Completion evidence:
  - Pending.

### T504: Pgvector ANN Search Support

- Status: [ ]
- Milestone: M5
- Risk: High
- Touched subsystems: backend, database, semantic-search, migrations
- Deliverables:
  - HNSW ANN indexes for image embeddings, with operator class selected by the
    model pack distance metric.
  - Exact-search fallback kept available for correctness checks and small
    libraries.
  - Search config for ANN enablement and scan breadth, with conservative
    defaults.
  - Filter-aware query plan for `model_pack_id`, `owner_id`, and
    `asset_trashed_at IS NULL`.
  - Regression tests proving ordering semantics for cosine, dot product, and
    L2 remain correct.
- Definition of done:
  - ANN is opt-in until recall/latency checks exist.
  - Index DDL matches the vector operator used by the query.
  - Mixed embedding dimensions are either blocked for ANN or handled with
    per-model/per-dimension partial or expression indexes.
  - Tests cover the negative-inner-product pgvector convention so callers still
    receive higher-is-better scores.
  - Explain-plan or catalog tests prove the intended ANN and filter indexes
    exist after migrations.
- Required gates:
  - `make gate-backend`
  - `make test-db`
  - Focused semantic-index DB tests.
- Caveats/footguns:
  - Pgvector applies filters after approximate scans in common plans; selective
    owner/trash filters can reduce recall unless candidate breadth and indexes
    are chosen deliberately.
  - HNSW improves query speed/recall but increases build time, memory, and
    row-by-row reindex cost.
  - Plain `vector` columns may need expression or partial indexes when model
    dimensions differ.
- Completion evidence:
  - Pending.

### T505: Model-Pack Authoring And Install Contract

- Status: [ ]
- Milestone: M5
- Risk: Medium
- Touched subsystems: backend, model-packs, docs, tooling
- Deliverables:
  - JSON Schema export for the model-pack manifest.
  - Small validator command that checks a local model-pack directory before
    install.
  - Operator-facing error messages for missing files, bad checksums, invalid
    runtime config, and failed self-tests.
  - Example semantic model-pack manifest fixture with fake tiny files suitable
    for tests.
- Definition of done:
  - A model-pack author can validate a directory without starting the API.
  - The CLI and HTTP install path use the same validation function.
  - Schema generation is derived from Rust types; no hand-written duplicate
    schema.
  - No model files are downloaded automatically by the backend.
- Required gates:
  - `make gate-backend`
  - Focused model-pack tests.
- Caveats/footguns:
  - Schema libraries do not replace domain validation; path, checksum, self-test,
    task-kind, and runtime invariants must remain in Rust.
  - Generated schema must not promise support for runtime backends we do not
    implement.
- Completion evidence:
  - Pending.

### T506: Face Pipeline Backend Foundation

- Status: [ ]
- Milestone: M5
- Risk: High
- Touched subsystems: backend, ml-worker, database, people
- Deliverables:
  - Tables for detected faces, face embeddings, people clusters, and user
    review state.
  - Worker job kind for face indexing that is disabled unless face recognition
    is enabled.
  - Model-pack task kinds for face detection and face embedding.
  - Pure clustering/review functions for merge, split, hide, rename, and
    unassigned faces.
- Definition of done:
  - Face data model preserves asset ownership boundaries and cascades on asset
    deletion.
  - Face recognition can be disabled without breaking semantic search.
  - No people cluster is exposed as user-trusted identity until reviewed or
    named by the owner.
  - Tests cover merge/split/hide invariants without requiring a real face model.
- Required gates:
  - `make gate-backend`
  - `make test-db`
  - Focused people/face DB tests.
- Caveats/footguns:
  - Face recognition is privacy-sensitive and must stay owner-local.
  - Do not mix face embeddings with semantic image/text embeddings.
  - Avoid building web/Android people UI in this task.
- Completion evidence:
  - Pending.

### T507: Upload And Object Integrity Worker

- Status: [ ]
- Milestone: M4
- Risk: Medium
- Touched subsystems: backend, uploads, storage, jobs, database
- Deliverables:
  - Background integrity check job for originals and derivatives.
  - Stored checksum verification against object storage.
  - Repair-safe failure state that reports missing/corrupt objects without
    deleting database rows automatically.
  - Admin endpoint or maintenance command to enqueue an integrity scan.
- Definition of done:
  - Corrupt or missing object data is detected and recorded.
  - Integrity scan is resumable/idempotent and safe to rerun.
  - The worker never deletes the only copy of user data as part of detection.
  - Tests cover missing object, checksum mismatch, and healthy object cases.
- Required gates:
  - `make gate-backend`
  - Focused storage/integrity tests.
- Caveats/footguns:
  - Do not make this a full repair system yet; detection plus clear reporting is
    enough for v1.
  - Object reads must stay bounded.
  - Backup/restore paths must not confuse staging files with durable originals.
- Completion evidence:
  - Pending.

### T508: Rate Limits For Expensive Backend Routes

- Status: [ ]
- Milestone: M2
- Risk: Medium
- Touched subsystems: backend, auth, rate-limits, uploads, search, model-packs
- Deliverables:
  - Persistent Postgres-backed limits on login/setup attempts, upload creation,
    semantic search, and model-pack admin actions.
  - Route-level limit keys that combine owner/session/IP where appropriate.
  - Tests for limit exhaustion and reset behavior.
- Definition of done:
  - Expensive or security-sensitive routes cannot be spammed by one session or
    one source IP.
  - Limits survive API process restart.
  - Limit failures return consistent `429` responses without leaking account
    existence.
  - No process-local limiter is introduced unless it proves necessary as a cheap
    prefilter.
- Required gates:
  - `make gate-backend`
  - Focused auth/rate-limit tests.
- Caveats/footguns:
  - Do not protect owner-only admin endpoints solely by rate limits; auth remains
    mandatory.
  - Be careful with reverse proxy IP headers; trust only configured proxy
    headers.
- Completion evidence:
  - Pending.

### T509: Backend API Contract Freeze For V1 Clients

- Status: [ ]
- Milestone: M4
- Risk: Medium
- Touched subsystems: backend, HTTP API, web, android, docs
- Deliverables:
  - OpenAPI or equivalent generated API description for implemented v1 routes.
  - Stable response envelopes and error shapes for auth, uploads, assets,
    sharing, search, model packs, and backup/maintenance status routes.
  - Contract tests for routes consumed by web and Android clients.
  - Explicit list of non-v1/internal endpoints.
- Definition of done:
  - Web and Android can generate or hand-write clients against one documented
    route contract.
  - Error responses are consistent enough for clients to show useful states.
  - Internal/admin routes are marked and require owner auth.
  - Existing backend tests assert the documented status codes and response
    shapes for critical routes.
- Required gates:
  - `make gate-backend`
  - Focused HTTP contract tests.
- Caveats/footguns:
  - Do not freeze endpoints that are known placeholders.
  - Keep generated docs derived from route/type definitions where practical to
    avoid hand-written drift.
  - Frontend styling and Material UI decisions are out of scope for this task.
- Completion evidence:
  - Pending.
