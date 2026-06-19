# Mirror V1 Implementation Plan

This plan controls implementation. The stable product and architecture source is
`docs/v1-architecture.md`; this document turns it into milestones, deliverables,
and done conditions.

## Operating Rules

- Each implementer or agent owns one task from `docs/tasks.md` at a time.
  Multiple tasks may run in parallel only when write scopes and dependencies
  are explicitly disjoint.
- Each task must record risk, touched subsystems, deliverables, definition of
  done, required gates, caveats, and completion evidence.
- Architecture drift is not allowed silently. If implementation needs a
  different product, security, module, or dependency decision, update the
  relevant design doc first.
- Warnings are signals. Investigate every compiler/linter warning and either
  fix it or document a task-linked suppression.
- Prefer small vertical slices over broad skeletons unless a skeleton is needed
  to unblock the slice.

## Risk Levels

- `Low`: isolated docs, tests, or UI copy with no behavior/security/data impact.
- `Medium`: ordinary feature code with clear rollback and no irreversible data
  impact.
- `High`: auth, upload promotion, storage, migrations, jobs, sharing, Android
  backup/delete, backups, rate limits, or cross-client contracts.
- `Critical`: changes that can cause data loss, auth bypass, public media
  exposure, backup/restore failure, token leakage, or irreversible local-device
  deletion.

Auth, storage, migrations, upload promotion, sharing, backup/restore, Android
deletion, and rate limits are never `Low`.

## Milestones

### M0: Implementation Control Docs

Deliverables:

- `docs/implementation-plan.md`
- `docs/tasks.md`
- `docs/quality-gates.md`
- `docs/module-boundaries.md`
- `docs/llm-workflow.md`
- `docs/caveats.md`

Definition of done:

- Docs are present and mutually consistent.
- Task template includes risk, deliverables, DoD, gates, caveats, and evidence.
- Initial tasks for M1-M5 exist.

### M1: Foundation And Login

Deliverables:

- Repo shape: `src/backend/`, `src/web/`, `src/android/`, `docs/`, `infra/`.
- Actix API skeleton with config, request IDs, health/readiness endpoints.
- Postgres migration setup and initial owner/session schema.
- One-time setup-token owner creation.
- Custom DB-backed web sessions and Android device-token login.
- Minimal React and Android login surfaces.

Definition of done:

- Setup token is required, single-use, and invalid after owner creation.
- Web login creates a DB-backed session.
- Android login creates a revocable hashed device token.
- Session/token tests and relevant gates pass.

### M2: Upload And Storage Kernel

Deliverables:

- OpenDAL local storage backend.
- First-party chunk upload protocol.
- Android selected-folder scan and upload worker.
- Original staging, BLAKE3 verification, content-addressed promotion.
- Asset/source records and Postgres job enqueue.

Definition of done:

- Android uploads a real photo from a selected folder.
- Backend verifies and promotes the original.
- Exact duplicate bytes do not duplicate stored originals.
- Metadata/derivative job is enqueued transactionally.

### M3: Media And Timeline

Deliverables:

- Metadata extraction.
- AVIF/WebP thumbnails and previews.
- Video posters.
- Timeline API.
- React timeline.
- Android timeline.

Definition of done:

- Uploaded photo appears in both web and Android timelines with a preview.
- Private share metadata behavior is not implemented yet, but stored metadata
  preserves GPS/full EXIF for owner use.
- Media fixture tests pass for supported formats available in the environment.

### M4: Safety, Sharing, Export, Backup

Deliverables:

- Private share links with privacy headers.
- Trash/restore/purge.
- Export originals plus manifest.
- Restic backup orchestration and restore check.
- Security hardening and rate limits.

Definition of done:

- Share links are revocable, optionally expiring, and do not expose GPS/full
  EXIF or people labels by default.
- Trash prevents accidental permanent deletion.
- Backup restores durable state into clean Postgres and empty storage.
- Security tests pass.

### M5: Optional ML

Deliverables:

- Optional local ML worker.
- Runtime-agnostic curated model-pack manifest.
- SigLIP2 semantic-search pack.
- People-album backend for owner-local face/people storage, ONNX face
  detection/alignment/embedding, clustering, and review flows.
- Reindex progress and model install state.

Definition of done:

- Semantic search works on fixture assets.
- People review flows for merge/split/name/hide/unassign pass backend tests,
  and the face-index job persists runtime-produced embeddings through the
  people pipeline.
- Model changes do not mix incompatible embedding revisions.

## Completion Evidence

Every completed task must add evidence in `docs/tasks.md`:

- Commands run and result.
- Tests added or updated.
- Manual verification, if applicable.
- Caveats added/closed.
- File paths touched.
