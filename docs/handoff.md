# Mirror Implementation Handoff

Last updated: 2026-06-15

## Current State

Worktree is intentionally dirty. Do not reset or revert existing changes.
Backend, web, and Android work are all under `src/`.

Completed slices:

- T106: real web logout and active-session inventory.
- T206: Android queue isolation, remote-generation fencing, bounded
  WorkManager batches, local-first disconnect, and Room migration 1-2.

Integrated, awaiting full Postgres evidence:

- T207: route-local 4 MiB upload-part limit and deterministic part framing.
- T208: original-object integrity scan and explicit orphan remediation.

Active slice:

- T301: final Postgres/deployment evidence for media worker.
- T302: Android dense timeline; web timeline is complete.

T301 and T302 remain incomplete.

## Implemented, Pending Final Evidence

### Web Sessions

- `/auth/logout` is called with the CSRF cookie mirrored into
  `x-csrf-token`.
- `/sessions` is rendered in a minimal session view.
- Failed logout keeps the authenticated UI visible.
- Web typecheck, lint, six tests, and build passed in the implementing agent.

### Android Backup Kernel

- Queue claims/counts/verification join selected and available folders.
- Coordinator rechecks eligibility before network work, between parts, and
  before completion.
- Backup rows carry a remote generation. URL changes clear local media
  progress; stale workers cannot mutate the new generation.
- Every worker run activates the current credential scope before scanning.
- Work is capped at four media items and appends continuation work.
- Local credentials clear before best-effort remote revoke.
- Room schema version 2 and explicit migration 1-2 exist.
- JVM tests, static analysis, lint, builds, and final API 36 instrumentation
  tests passed.

### Upload Framing

- Actix reads upload parts through a route-local 4 MiB bound.
- Oversized bodies return JSON `413 upload_part_too_large`.
- Part index and exact deterministic length are validated before object or DB
  writes.
- Completion validates contiguous framed parts.
- Focused upload tests passed; full backend/DB gates remain to be rerun after
  integration.

### Public Asset Identity

- Upload completion now returns `assets.public_id`, matching timeline and
  derivative routes.
- DB-oriented media tests resolve the internal asset row ID explicitly.

### Storage Integrity

- `integrity::scan_original_storage` reports:
  - storage objects without an `originals` row;
  - `originals` rows whose object is missing.
- Scan is read-only.
- Explicit remediation accepts only canonical
  `originals/blake3/aa/bb/<64-hex>` keys and rechecks Postgres immediately
  before deletion.
- `maintenance` is dry-run by default. `--apply` requires selected orphan keys.
- Focused tests exist for both mismatch directions, selective deletion,
  canonical-key rejection, and recheck-before-delete.

### Media Worker

- Still images are rejected before storage reads above 512 MiB.
- Decoder limits cap dimensions at 32768 and allocations at 256 MiB.
- Pinned `nom-exif` extracts bounded owner-only camera, capture-time, GPS, and
  raw EXIF entries.
- Missing, unsupported, or malformed optional metadata is classified without
  failing otherwise valid image processing.
- JPEG fixture tests prove GPS/camera extraction and metadata-free generated
  WebP derivatives.
- Video originals stream into private bounded temp files; they are not loaded
  fully into RAM.
- `ffprobe`/`ffmpeg` commands have kill deadlines, bounded output, direct
  argument passing, and metadata-free WebP poster output.
- Video metadata includes duration and rotation-correct display dimensions.
- Worker heartbeats retain leases. Wall timeout records retry and exits the
  worker process to terminate stuck parser threads.
- V1 derivative format is WebP only; AVIF is intentionally deferred.
- `make gate-backend` passed after video and timeout integration.

### Web Timeline

- Cursor pages append through `useInfiniteQuery`.
- TanStack Virtual bounds mounted rows for large timelines.
- Scroll-near-end loading has a visible load-more fallback.
- Responsive dense grid shows video markers and opens a keyboard-navigable
  full-screen preview.
- Unit tests moved to `src/web/tests`; no web tests remain under implementation
  source.
- Playwright uses system Chromium and verifies pagination, bounded DOM,
  preview, desktop layout, and 390x844 mobile layout.
- Vite upgraded to 8.0.16; `npm audit` reports zero vulnerabilities.
- `make gate-web` passed: typecheck, lint, seven unit tests, Playwright, build.

## Immediate Next Steps

1. Start Docker Desktop, then run all opt-in Postgres tests:

   ```sh
   rtk make db-up
   rtk make test-db
   ```

2. Complete T301:
   - fix any opt-in DB failures;
   - add non-root worker container and CPU/memory/temp-disk limits;
   - add HEIC/HEIF only behind a tested host capability path.

3. Start T302 timeline/detail behavior only after T301 derivative contracts
   settle. Web is complete; implement Android dense timeline and preview.

## Known Constraints

- C001 remains fundamentally true: Postgres and object storage cannot commit
  atomically. Integrity tooling mitigates detection/remediation; it does not
  remove the boundary.
- C018: Android scopes a remote vault by normalized server URL. Same-URL vault
  replacement needs a stable backend instance ID later.
- Android cleartext remains process-wide in the manifest because arbitrary
  private LAN IPs cannot be represented statically. `ServerEndpoint` is the
  required runtime guard.
- Test source roots under `src/android/tests` and
  `src/android/tests-instrumentation` are intentional project policy.
- Migration/device tests are opt-in; default root gate must not require an
  emulator or Postgres.
- C019: deployment proxy examples must accept 4 MiB parts plus request
  overhead; Actix retains the exact application limit.

## Environment

- JDK used by Android build: 17.
- Installed Android SDK: platform/build-tools 36, platform-tools 37, API 36
  Google APIs x86_64 system image.
- AVD name: `MirrorApi36`.
- Debug APK:
  `src/android/app/build/outputs/apk/debug/app-debug.apk`.
- Local Postgres compose service uses `infra/compose.yaml`.
- Docker Desktop daemon was stopped at last verification, so the newest
  T207/T208/T301 database suite has not run.
