# Mirror V1 Architecture

## Document Purpose

This document is the canonical v1 product and architecture reference for Mirror.
It is a guardrail, not a brainstorm. It defines what the product is, what it is
not, and which architectural abstractions are allowed so implementation does not
drift into a generic cloud drive, social app, plugin platform, or premature
distributed system.

Mirror v1 is a self-hosted photo vault for personal photos and phone videos.

## Product Boundary

Mirror is built around a single owner preserving, finding, organizing, sharing,
and exporting their photo and phone-video library.

The product is intentionally not a general-purpose file manager. All domain
concepts should remain photo-vault concepts unless there is a concrete v1 need
for broader language.

Core nouns:

- Asset: a logical photo or video in the library.
- Original: the immutable uploaded/imported media file.
- Source: where an asset came from, such as Android backup, web upload, or
  server import.
- Derivative: a reproducible thumbnail, poster, preview, or playback proxy.
- Album: an owner-managed grouping of assets.
- Share: a revocable private link to assets or an album.
- Face: a detected face occurrence inside an asset.
- Person: an owner-named cluster of face occurrences.

## V1 Positive Space

V1 includes:

- Single-owner vault with owner password, optional/recommended TOTP, recovery
  codes, web sessions, and Android device tokens.
- Android folder-based backup.
- Web drag/drop upload.
- Server-side import that copies files into managed vault storage.
- Photos and phone videos as first-class media.
- Original preservation with reproducible thumbnails, metadata, video posters,
  and optional playback proxies.
- Timeline, albums, favorites, metadata search, semantic image-content search,
  named people albums, trash, and export.
- Revocable private share links with optional expiry and optional original
  download.
- Hybrid Android storage: use local originals when still present, fetch
  server-only originals on demand, and cache thumbnails/metadata.
- Guarded Android "free up space" after verified backup.
- Optional local ML with user-enabled model installation and reindex progress.
- Built-in restic backups for durable vault state.

## V1 Negative Space

V1 deliberately excludes:

- Generic file drive, document vault, notes, contacts, calendar, music, or
  arbitrary blob management.
- Multi-user household accounts, SaaS multi-tenancy, public profiles, comments,
  likes, feeds, collaborative uploads, or social graph.
- End-to-end encryption, passkeys, OAuth/social login, LDAP/SAML, role policy
  engine, or advanced admin delegation.
- iOS app, desktop app, CLI product, WebDAV, or PWA offline-first experience.
- Index-in-place import, two-way filesystem sync, NAS file-manager behavior, or
  automatic device cleanup.
- RAW-first workflow, photo/video editing, Live Photo pairing, advanced
  transcoding controls, sidecar writing, or map/geocoding.
- AI captions, auto labels, age/gender/emotion inference, liveness detection,
  surveillance/watchlist search, uploaded-face search, or near-duplicate
  cleanup.
- Redis, Qdrant, Kubernetes-first design, plugin system, event bus, microservice
  split, or generic extension framework unless later evidence proves v1 cannot
  meet its reliability goals without one.

## Drift Guardrails

- A v1 feature must directly support ingesting, preserving, finding,
  organizing, sharing, exporting, or safely deleting photos/videos.
- Prefer concrete photo-vault concepts over generic abstractions: `asset`,
  `album`, `person`, `face`, `share`, and `original`, not `object`, `node`,
  `collection`, or `actor`.
- Do not add an abstraction unless v1 has two real implementations, except for
  storage backends and ML runtimes, which are intentional extension points.
- Originals are immutable. Metadata, albums, people labels, shares, and trash
  state are mutable.
- Derivatives are disposable and reproducible. Originals are never disposable
  except through explicit purge.
- All long-running or side-effectful work goes through idempotent background
  jobs.
- Keep application structure boring: feature modules, direct SQLx queries, and
  explicit functions. Do not introduce clean architecture, onion architecture,
  generic repositories, service containers, actor systems, or event buses in v1.
- Web admin is canonical for setup, import, export, storage settings, model
  settings, sessions, and recovery. Android has daily-use parity, not full admin
  parity.
- Internal OpenAPI is stable for first-party clients only. V1 does not promise
  a public SDK or long-term third-party API compatibility.

## Repo And Deployment Shape

Recommended repository layout:

```text
src/backend/
src/web/
src/android/
docs/
infra/
```

Deployment is Docker Compose first:

- `api`: Rust HTTP API and static web hosting if bundled.
- `worker`: Rust media/import/export/job worker.
- `ml-worker`: optional Rust ML worker.
- `postgres`: system of record with `pgvector`.
- Optional S3-compatible object storage, such as MinIO, for deployments that do
  not use local filesystem storage.

Do not design v1 around Kubernetes, independent microservice deployment,
distributed workers, service discovery, or multi-node high availability.

First implementation sequence:

1. Repo restructure, Actix API skeleton, config, Postgres migrations, health
   checks, one-time setup-token owner creation, custom DB sessions, and basic
   web/Android login.
2. OpenDAL local storage, first-party chunk upload, Android folder scan/upload,
   original promotion, asset/source records, and Postgres jobs.
3. Metadata extraction, image previews, video posters, timeline API, React
   timeline, and Android timeline.
4. Sharing, trash, exports, durable restic backups, security hardening, and
   rate-limit hardening.
5. Semantic search and people albums with the optional local ML worker.

The first milestone must be a backend + web + Android vertical slice:
setup/login, Android folder upload, backend storage/jobs, and web/Android
timeline display of the uploaded asset.

## Backend Architecture

Use a Rust modular monolith with separate binaries for `api`, `worker`, and
optional `ml-worker`. The codebase should be modular, but deployment and data
ownership remain simple.

Recommended core crates and tools:

- `actix-web` for HTTP routing.
- `tokio` for async runtime.
- `sqlx` for Postgres access and migrations.
- `utoipa` or an equivalent OpenAPI generator for first-party client contracts.
- `tracing` and structured JSON logs.
- `uuid` with UUIDv7 for time-sortable public IDs.
- `blake3` for content hashing.
- `opendal` for local filesystem and S3-compatible storage backends.

Actix companion crates:

- Use `tracing-actix-web` for request IDs and structured request spans.
- Use `actix-cors` only for split-origin development or advanced deployments.
- Use `actix-files` if the API process serves the React/Vite static build.
- Consider `actix-web-httpauth` for bearer token parsing only; keep
  authorization and device-token validation custom.
- Do not use `actix-session` or `actix-identity` for core auth. Their default
  stores and string-map session model take away control that v1 needs for
  revocation, audit, session inventory, and Postgres-only deployment.

Backend modules:

- `auth`: owner setup, login, TOTP, recovery codes, sessions, device tokens.
- `assets`: logical assets, originals, sources, favorites, trash state.
- `uploads`: resumable upload sessions and verification.
- `storage`: local filesystem and S3-compatible object storage abstraction.
- `media`: metadata extraction, thumbnails, posters, video proxies.
- `albums`: manual albums and album membership.
- `shares`: private link creation, revocation, expiry, and download policy.
- `search`: metadata search and semantic search orchestration.
- `people`: face occurrences, clusters, person labels, merge/split/hide flows.
- `models`: model pack catalog, install state, active model revisions.
- `jobs`: durable background job queue.
- `exports`: export originals plus metadata manifests.
- `audit`: security and admin activity log.
- `sync`: monotonic client sync events for Android and web cache refresh.
- `limits`: security-sensitive rate limits and cooldowns.

Postgres is the system of record. Use SQL migrations and explicit schema
ownership. Avoid hidden state in worker-local files except caches and temporary
staging data.

Default call flow:

```text
Actix handler -> feature module function -> sqlx/storage/jobs
```

Use traits only where v1 has a concrete reason: storage backend, ML runtime,
clock/id generation in tests, and narrow interfaces around external media
tools. Do not add generic repositories or use-case classes.

## Data Model

The schema should preserve these separations:

- Logical asset vs physical original bytes.
- Original bytes vs reproducible derivatives.
- Asset metadata vs owner organization metadata.
- Private library metadata vs share-page presentation.
- Semantic asset embeddings vs face identity embeddings.
- Current active model revision vs historical embeddings from previous models.

Core tables:

- `owner_accounts`
- `sessions`
- `device_tokens`
- `assets`
- `originals`
- `asset_sources`
- `derivatives`
- `albums`
- `album_assets`
- `shares`
- `share_assets`
- `trash_entries`
- `upload_sessions`
- `import_runs`
- `jobs`
- `audit_events`
- `sync_events`
- `rate_limit_buckets`
- `model_packs`
- `model_installations`
- `asset_embeddings`
- `face_occurrences`
- `face_embeddings`
- `people`
- `person_faces`
- `exports`

Use UUIDv7 for externally visible IDs. Database integer IDs are allowed
internally only when they do not leak into public API contracts.

## Jobs And Reliability

Use a custom SQLx/Postgres durable job queue in v1.

A "job enqueuer" is only a small helper in the `jobs` module that inserts job
rows in the same database transaction as the domain change that requires
background work. It is not an event bus, message broker, actor system, or
separate service abstraction.

Job requirements:

- `FOR UPDATE SKIP LOCKED` leasing.
- Worker heartbeat and lease expiry.
- Idempotency keys for all side-effectful jobs.
- Retry count, exponential backoff, and dead-letter state.
- Priority and run-after scheduling.
- Structured error payloads for admin diagnostics.

Typical jobs:

- Finalize upload.
- Import directory scan.
- Import file copy.
- Extract metadata.
- Generate image derivatives.
- Generate video poster/proxy.
- Install model pack.
- Embed asset for semantic search.
- Detect/embed faces.
- Cluster faces.
- Export library.
- Purge trash.

Do not use Redis, Fang, PGMQ, RabbitMQ, Kafka, SQS, or a task framework in v1.
Revisit only if measured workload shows the custom Postgres queue is the
bottleneck or lacks a required reliability property.

## Storage And Media Kernel

Storage backends:

- Local filesystem default through OpenDAL.
- S3-compatible object storage through OpenDAL.

Upload/import flow:

1. Create upload or import session.
2. Write bytes into staging storage.
3. Stream-hash with BLAKE3.
4. Validate size, media signature, and supported type.
5. Promote atomically to content-addressed original storage.
6. Create or attach logical asset/source records.
7. Enqueue metadata and derivative jobs.

Original storage rules:

- Originals are immutable after promotion.
- Originals are addressed by content hash.
- Exact duplicate bytes do not create duplicate stored originals.
- Logical duplicate policy is separate from byte dedupe; v1 should avoid smart
  near-duplicate cleanup.

Derivative rules:

- Derivatives are reproducible and may be deleted/regenerated.
- Store derivative kind, size, format, source original hash, generator version,
  and storage key.
- Request handlers never do expensive media processing inline.
- Generate AVIF/WebP thumbnails and previews plus video posters in v1.
- Generate video playback proxies only when browser/device playback requires
  them; do not always transcode every video by default.

Metadata privacy:

- Extract and store EXIF/GPS metadata for owner search, owner detail views, and
  export.
- Private share pages never expose GPS or full EXIF by default.
- Previews strip sensitive metadata while originals preserve it.

Media tooling:

- Use Rust `image` for v1 still-image thumbnails/previews. Keep the processor
  behind a trait so libvips can replace it later if profiling shows need.
- Use `ffmpeg`/`ffprobe` for video inspection, posters, and optional playback
  proxies.
- Prefer Rust metadata parsing such as `nom-exif` for owner metadata. Keep an
  external `exiftool`-style wrapper as a possible fallback for formats the Rust
  parser does not cover well enough.

Supported v1 media:

- JPEG
- PNG
- WebP
- HEIC/HEIF when host tooling supports it
- MP4
- MOV

SVG and active web content are unsupported uploads.

## API Design

Use REST JSON APIs with generated OpenAPI.

Main resources:

- `/auth`
- `/sessions`
- `/devices`
- `/uploads`
- `/assets`
- `/albums`
- `/shares`
- `/search`
- `/people`
- `/jobs`
- `/models`
- `/exports`
- `/sync`

API rules:

- Cursor pagination for timeline/search/albums.
- Stable error envelope with machine-readable code and human-readable message.
- Explicit optimistic concurrency fields for mutable user metadata when needed.
- OpenAPI is a contract and documentation artifact; web and Android clients are
  hand-written, not generated.
- No public third-party compatibility guarantee in v1.
- No GraphQL in v1.

Upload protocol:

- Use a first-party resumable chunk protocol.
- `POST /uploads` creates an upload session with expected file metadata.
- `PUT /uploads/{id}/parts/{part_index}` writes staged parts.
- `GET /uploads/{id}` returns committed parts and current status for resume.
- `POST /uploads/{id}/complete` verifies size/hash, promotes the original, and
  creates durable asset state.
- `DELETE /uploads/{id}` cancels an unfinished upload session.

Sync API:

- Write a `sync_events` row for asset, album, person, trash, and relevant model
  state changes.
- Android consumes events with `/sync/events?after=<sequence>`.
- Sync events are for first-party cache refresh and backup state only, not a
  public webhook/event-bus feature.

## Search And ML

Semantic search and face recognition are separate subsystems with shared
infrastructure.

Shared ML infrastructure:

- Optional local `ml-worker`.
- User-enabled model installation.
- Model catalog visible from web and Android.
- Reindex progress visible in clients.
- Runtime-agnostic curated model packs.
- Approved runtimes begin with ONNX Runtime and Candle/safetensors.
- Advanced user-supplied models are allowed only if they provide a compatible
  manifest and pass self-tests.

Model pack manifests declare:

- Kind: `semantic_image_text` or `face_identity`.
- Runtime.
- Model key and pinned revision.
- File list and checksums.
- License.
- Input sizes and preprocessing.
- Embedding dimension.
- Distance metric.
- Thresholds.
- Golden self-test fixtures.

Semantic search:

- Default model family: SigLIP2 Base.
- Index one embedding per photo asset.
- Index one poster-frame embedding per video in v1.
- Text queries embed on demand.
- Ranking uses vector similarity plus normal SQL filters such as date, album,
  media type, favorite, and trash state.

People albums:

- Default model pack: AuraFace.
- Conservative fallback: OpenCV YuNet/SFace.
- Pipeline: detect face, align/crop, embed, cluster, let owner name/merge/split
  or hide clusters.
- Face recognition is user-enabled and can be disabled independently from
  semantic search.
- People labels are private library metadata and are not exposed in share pages
  by default.

Model changes:

- Embeddings from different models are incompatible.
- Store embeddings by `model_key` and `model_revision`.
- Keep the old active index while a new one builds.
- Switching people models requires a people-index rebuild and owner review.

## Web App Architecture

Use React, Vite, and TypeScript. The web app is a static SPA served by the Rust
API container or a reverse proxy. Do not run a Node production server in v1.

Web responsibilities:

- Owner setup and login.
- Timeline browsing.
- Asset detail viewer.
- Search, including semantic search and people filters.
- Albums and favorites.
- People albums and cluster review.
- Uploads.
- Shares.
- Trash.
- Imports.
- Exports.
- Sessions and device tokens.
- Model install/enable/reindex status.
- Storage settings and admin diagnostics.

Implementation rules:

- Use TanStack Router for typed routes and URL search-param state.
- Use TanStack Query for server state.
- Use TanStack Virtual for timeline/search grids.
- Keep a manual TypeScript API client under `src/web/src/api`; mirror OpenAPI DTOs
  and cover drift with contract tests.
- Use virtualized grids for timeline and search.
- Keep full-size originals out of persistent browser caches.
- Browser offline support is not a v1 feature.
- Do not add Redux, a custom app-wide event bus, SSR, or service-worker offline
  mode in v1.
- Avoid marketing-style landing pages; the app opens into setup, login, or the
  library.

Upload manager:

- Slice files into first-party upload parts.
- Limit concurrent part uploads.
- Persist pending upload sessions in IndexedDB.
- Resume by querying upload session status.
- Show duplicate, verification, and retry states explicitly.

## Android App Architecture

Use Kotlin and Jetpack Compose.

Core Android components:

- Compose UI.
- Room local cache.
- WorkManager for durable backup jobs.
- MediaStore for selected-folder scanning and local file state.
- Encrypted storage for device token.
- Ktor Client with `kotlinx.serialization`.
- Coil for image loading in Compose.

Android responsibilities:

- Login and device registration.
- Folder selection for backup.
- Backup status and retry.
- Timeline browsing.
- Asset viewer.
- Search, albums, favorites, people.
- Create/revoke private share links.
- Show model install/indexing state and allow enabling optional ML.
- Guarded "free up space".

Release target:

- V1 optimizes for modern sideload/APK distribution before Play Store polish.
- Target the current Android SDK.
- Default `minSdk` to Android 10/API 29 unless implementation testing proves a
  better cutoff.

Architecture rules:

- Use feature packages with `ui`, `data`, and `domain` subpackages only where
  they simplify a real feature. Do not create a generic clean-architecture
  template.
- ViewModels expose immutable `StateFlow` UI state.
- Repositories coordinate Room plus the manual `MirrorApi` client.
- `MirrorApi` owns auth headers, error decoding, upload streaming, and retry
  classification.
- Room stores remote asset cache, local media links, upload queue, backup
  folders, sync cursor, people/albums cache, and download cache metadata.

UI direction:

- Use restrained Material 3 with a quiet photo-vault theme.
- Open into a full-screen timeline, not a dashboard.
- Use a dense adaptive photo grid with stable item dimensions and month/day
  section headers.
- Use three or four bottom-navigation destinations at most.
- Use a black immersive asset viewer with metadata/actions behind gestures or
  sheets.
- Keep backup status small but easy to find.
- Avoid decorative cards, oversized empty states, and marketing-style surfaces.

Folder backup:

- Default to Camera.
- Let owner add folders such as Screenshots, Downloads, or messaging app media.
- Wi-Fi-only default.
- Charging-only and cellular options may be settings, not required defaults.
- Uploads are verified by the server before marked backed up.
- Request broad photo/video access because backup is core. If Android grants
  partial media access, treat backup as degraded/partial and make that visible.
- Do not request all-files access in v1.

Local/cloud file behavior:

- If a local original is still present and matches known state, open local file.
- If an asset is server-only, show cached metadata/thumbnail and fetch original
  on demand.
- Full offline originals are explicit downloads, not automatic whole-vault
  mirroring.

Free up space:

- Only offer for verified, unchanged, non-trashed local media.
- Use Android's explicit media delete flow.
- Make clear that local originals are deleted while vault originals remain.
- Do not automatically delete local files by age or policy in v1.

## Security Model

V1 is server-trusted. The server can read originals and metadata.

Threat model:

- Mirror may be exposed to the internet through a reverse proxy such as Caddy,
  Traefik, or nginx.
- The reverse proxy terminates public HTTPS in production.
- Mirror trusts forwarded host/proto/IP headers only from explicitly configured
  proxy IP ranges.
- A local/LAN install may use HTTP only when the client explicitly opts into an
  insecure private-address server. Android must never silently allow HTTP for
  public IPs or domains.
- The server, database, configured storage backend, and ML/media workers are
  inside the trusted boundary. This is not an end-to-end encrypted v1.

Authentication:

- First-run owner setup requires a one-time setup token printed in server logs
  and exposed through startup/admin output. An unclaimed internet-exposed
  instance must not be claimable without this token.
- The setup token is single-use and is invalid after owner creation.
- Argon2id password hashing.
- Optional quiet TOTP. V1 supports TOTP but does not force enrollment or
  aggressively nag.
- Recovery codes.
- No forced password rotation.
- Allow long passwords and password-manager paste.
- Sensitive actions require recent password reauthentication: changing backup
  secrets, purging trash, exporting originals, creating Android device tokens,
  revoking all sessions, changing model sources, and restoring backups.
- Custom DB-backed web sessions with opaque random cookie tokens.
- Store only hashed session tokens in Postgres.
- Track session expiry, revocation, last seen time, browser/device metadata, and
  audit events.
- Web cookies are `HttpOnly`; they are `Secure` when served over HTTPS and
  `SameSite=Lax` or stricter.
- CSRF protection uses a signed double-submit or synchronizer-token pattern for
  cookie-authenticated unsafe requests. `SameSite` is defense-in-depth, not the
  only CSRF control.
- Revocable Android device tokens.
- Store only hashed device tokens in Postgres.

Rate limiting:

- Rate limiting is enabled by default and configurable.
- Use two layers:
  - A process-local GCRA/token-bucket limiter for cheap burst protection on
    broad request classes.
  - A SQLx/Postgres-backed limiter for security-sensitive actions where
    persistence matters.
- Prefer using `governor` directly for the process-local limiter. Do not depend
  on `actix-governor` in v1 because its current package metadata shows a
  GPL-3.0-or-later license, which is avoidable by integrating `governor`
  ourselves.
- Implement the Postgres limiter in the `limits` module with explicit SQLx
  queries over `rate_limit_buckets`; do not add Redis or an obscure
  rate-limiter framework for v1.
- Postgres-backed limits cover login attempts, TOTP attempts, recovery-code
  attempts, share-token access, upload-session creation, export triggers, backup
  triggers, and restore triggers.
- Reverse-proxy rate-limit examples are documented as an extra outer layer for
  exposed installs.

Sharing:

- Share tokens are high-entropy random values.
- Store only hashed share tokens.
- Shares are scoped to specific assets or albums.
- Shares are revocable.
- Shares may expire.
- Original download is an explicit share setting.
- Share pages send `Referrer-Policy: no-referrer` and `X-Robots-Tag: noindex`.
- Share pages include no third-party scripts or analytics in v1.
- People labels are hidden from share pages by default.

Upload safety:

- Validate media signatures, not only MIME types or file extensions.
- Uploads are allowlist-only by signature and supported extension.
- Store uploads outside any writable web root.
- Stream downloads through authorization checks.
- Strip sensitive metadata from previews while preserving originals.
- Do not serve user-provided active content.
- User-controlled file names are display metadata only. Storage keys are
  generated and content-addressed.

Media, ML, and model supply chain:

- Media and ML workers run as non-root containers.
- Worker containers use a read-only app filesystem, constrained temp
  directories, CPU/memory limits, and no direct public serving.
- Media tools run with timeouts and bounded input/output paths.
- Model packs require pinned revisions, checksums, license metadata, and golden
  self-tests before activation.
- Advanced user-supplied model packs must pass the same manifest and self-test
  validation as curated packs.

Backups and secrets:

- Built-in backups orchestrate proven tools instead of custom backup crypto.
- Backup flow: create a `pg_dump -Fc` database dump, snapshot the dump plus
  vault storage with restic, record the backup manifest, then run an integrity
  check.
- Backups include durable state only: database dumps, originals, derivatives,
  model packs, exports/manifests, and settings.
- Backups exclude temp directories, staging parts, incomplete uploads, logs, and
  worker scratch data.
- Backup encryption secret comes from a mounted secret file. It is not stored in
  Postgres and is not editable through normal app UI.
- Admin must keep an offline copy of the restic password. Mirror warns if no
  successful restore check has been recorded.
- Restore flow is documented and tested: clean Postgres plus empty storage,
  restore restic snapshot, run `pg_restore`, then run a vault integrity scan.
- Secrets are loaded from environment variables or mounted secret files.
- Logs, diagnostics, errors, and audit payloads redact passwords, TOTP seeds,
  recovery codes, session tokens, device tokens, share tokens, backup secrets,
  and object-store credentials.

Audit events:

- Login/logout.
- Failed login.
- TOTP/recovery changes.
- Device token creation/revocation.
- Share creation/revocation/access when practical.
- Delete, restore, purge.
- Import/export.
- Model install/reindex/change.
- Backup create, backup check, restore start, restore success, and restore
  failure.

## Test Plan

Backend tests:

- First-run setup token is required, single-use, invalid after owner creation,
  and safe behind a reverse proxy.
- Upload session create/resume/complete.
- Hash and size verification.
- Exact byte dedupe.
- Import copy from allowlisted roots.
- Trash, restore, and purge.
- Share authorization, expiry, revocation, and download policy.
- Export originals plus manifest.
- Password login, TOTP, recovery codes, custom DB sessions, logout-all,
  session revocation, CSRF, and device tokens.
- Secure cookie attributes and CSRF success/failure cases.
- Trusted proxy header handling.
- Process-local burst limits and SQLx/Postgres security limits.
- Login, TOTP, recovery-code, share-token, upload-create, export, backup, and
  restore rate-limit cases.
- Job leases, retries, idempotency, dead-letter state.
- Transactional job enqueue rollback.
- Sync event creation and cursor consumption.
- Secret redaction in logs, errors, diagnostics, and audit payloads.

Storage tests:

- Contract tests against local filesystem.
- Contract tests against MinIO/S3-compatible storage.
- Atomic promotion and missing-object recovery behavior.
- Original objects are never publicly accessible without app authorization.

Media tests:

- JPEG, PNG, WebP, HEIC/HEIF, MP4, and MOV fixtures.
- Metadata extraction.
- Thumbnail generation.
- Video poster generation.
- Unsupported SVG/active content rejection.
- Worker timeout and bounded temporary-directory behavior.
- Owner metadata views retain GPS/full EXIF while private share pages omit
  GPS/full EXIF by default.

ML tests:

- Model pack manifest validation.
- Model checksum validation.
- Golden embedding dimension tests.
- Semantic search fixture ranking.
- Face detection/embedding fixture tests.
- People cluster merge, split, name, and hide flows.
- Reindex behavior across model revisions.
- Model-pack checksum, license, pinned revision, and golden self-test
  enforcement.

Web tests:

- Setup/login.
- Timeline virtualization.
- Upload progress/failure/retry.
- Search and filters.
- Albums and favorites.
- People cluster review.
- Share creation/revocation.
- Trash.
- Import/export/model status screens.
- Manual API client contract coverage against OpenAPI examples.
- Share page `noindex`, referrer policy, and no third-party script checks.

Android tests:

- MediaStore folder scanning.
- WorkManager retry/resume.
- Verified backup state.
- Local-original vs server-only asset behavior.
- On-demand original fetch.
- Guarded free-space deletion.
- Device token revocation handling.
- Dense grid scroll performance and image-cache behavior on release builds.
- Sync cursor handling and stale-cache refresh.
- HTTPS-required behavior for public origins and explicit insecure-LAN opt-in.
- Modern sideload/APK release build behavior on the selected `minSdk`.

Backup tests:

- Restic backup creation from database dump plus vault storage.
- Durable-state boundary: DB dumps, originals, derivatives, model packs,
  exports/manifests, and settings are included; temp, staging, incomplete
  uploads, logs, and worker scratch are excluded.
- Restore into clean Postgres and empty storage.
- Post-restore vault integrity scan.
- Missing backup secret, wrong backup secret, and failed integrity-check
  reporting.

Vertical slice tests:

- Android uploads a real photo from a selected folder.
- Backend verifies and promotes the original, creates asset/source records, and
  enqueues metadata/derivative jobs.
- Worker creates a preview.
- Web and Android timelines both show the uploaded asset.

## References

- Immich architecture: <https://docs.immich.app/developer/architecture/>
- Immich mobile backup: <https://docs.immich.app/features/mobile-backup/>
- PhotoPrism metadata/search: <https://docs.photoprism.app/index.html>
- Ente architecture and future E2EE reference: <https://ente.photos/architecture/>
- SigLIP2: <https://arxiv.org/abs/2502.14786>
- FaceNet: <https://arxiv.org/abs/1503.03832>
- ArcFace: <https://arxiv.org/abs/1801.07698>
- pgvector: <https://github.com/pgvector/pgvector>
- Android WorkManager: <https://developer.android.com/guide/background/persistent/getting-started/define-work>
- OWASP file upload guidance: <https://cheatsheetseries.owasp.org/cheatsheets/File_Upload_Cheat_Sheet.html>
- OWASP ASVS: <https://owasp.org/www-project-application-security-verification-standard/>
- OWASP authorization guidance: <https://cheatsheetseries.owasp.org/cheatsheets/Authorization_Cheat_Sheet.html>
- OWASP CSRF guidance: <https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html>
- OWASP REST security guidance: <https://cheatsheetseries.owasp.org/cheatsheets/REST_Security_Cheat_Sheet.html>
- NIST SP 800-63B: <https://pages.nist.gov/800-63-4/sp800-63b.html>
- Actix Web: <https://actix.rs/docs/>
- Actix extras: <https://github.com/actix/actix-extras>
- Actix session crate considered but not used for core auth: <https://docs.rs/actix-session/latest/actix_session/>
- Actix identity crate considered but not used for core auth: <https://docs.rs/actix-identity/latest/actix_identity/>
- governor rate limiter: <https://docs.rs/governor/latest/governor/>
- governor state store API: <https://docs.rs/governor/latest/governor/state/trait.StateStore.html>
- actix-governor considered but not used due current package license:
  <https://docs.rs/crate/actix-governor/latest/source/Cargo.toml.orig>
- restic: <https://restic.net/>
- PostgreSQL `pg_dump`: <https://www.postgresql.org/docs/current/app-pgdump.html>
- OpenDAL: <https://opendal.apache.org/>
- Vite: <https://vite.dev/guide/>
- TanStack Router: <https://tanstack.com/router/latest/docs/framework/react/overview>
- TanStack Query: <https://tanstack.com/query/latest>
- Android Material 3: <https://developer.android.com/jetpack/androidx/releases/compose-material3>
- Android Compose image loading: <https://developer.android.com/develop/ui/compose/quick-guides/content/load-images>
