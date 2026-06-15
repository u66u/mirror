# Mirror Caveats And Footguns

This register tracks decisions, risks, and side effects that are easy to forget.
Tasks should reference caveat IDs when they touch related areas.

Status values:

- `Open`
- `Mitigated`
- `Accepted`
- `Closed`

## C001: DB And Storage Atomicity

- Status: Open
- Risk: Critical
- Related tasks: T201, T202, T204, T402
- Caveat: Postgres transactions cannot atomically commit object-storage writes.
  Upload promotion can leave orphan staged or promoted objects if the process
  dies between storage and DB operations.
- Mitigation: Design idempotent promotion, staged object cleanup, and integrity
  scans before treating upload promotion as complete.

## C002: Setup Token Exposure

- Status: Open
- Risk: High
- Related tasks: T104
- Caveat: First-run setup token must be visible enough for the admin but should
  be treated as sensitive until owner creation.
- Mitigation: Single-use token, invalid after owner creation, do not persist raw
  token beyond startup state unless hashed.

## C003: Trusted Proxy Headers

- Status: Open
- Risk: High
- Related tasks: T102, T105, T403
- Caveat: Trusting `X-Forwarded-*` or `X-Real-IP` from arbitrary clients enables
  IP spoofing, bad secure-cookie behavior, and rate-limit bypass.
- Mitigation: Trust forwarded headers only from configured proxy IP ranges.

## C004: Process-Local Rate Limits Are Not Persistent

- Status: Open
- Risk: High
- Related tasks: T403
- Caveat: Governor/process-local limits reset on restart and do not coordinate
  across API processes.
- Mitigation: Use SQLx/Postgres limits for security-sensitive actions.

## C005: Media Parser Attack Surface

- Status: Open
- Risk: High
- Related tasks: T301
- Caveat: Image/video/metadata parsers process attacker-controlled bytes.
- Mitigation: Non-root workers, read-only app filesystem, bounded temp dirs,
  timeouts, resource limits, and no public serving from worker paths.

## C006: HEIC/HEIF Host Dependency

- Status: Open
- Risk: Medium
- Related tasks: T301
- Caveat: HEIC/HEIF support depends on host libraries/codecs and may vary by
  container image or distribution.
- Mitigation: Treat support as capability-detected; tests should skip with a
  clear message if unsupported by the environment.

## C007: Android Media Permissions Drift

- Status: Open
- Risk: High
- Related tasks: T203
- Caveat: Android media permissions differ by OS version and user choice.
  Partial access can make backup incomplete.
- Mitigation: Make partial/degraded backup visible; do not request all-files
  access in v1.

## C008: Free Up Space Can Delete Local Originals

- Status: Open
- Risk: Critical
- Related tasks: T402
- Caveat: Android free-space behavior can remove the user's local original.
- Mitigation: Only verified, unchanged, non-trashed media is eligible; use
  Android explicit delete flow and clear confirmation.

## C009: Backup Key Loss

- Status: Open
- Risk: Critical
- Related tasks: T402
- Caveat: Losing the mounted restic password makes encrypted backups
  unrecoverable.
- Mitigation: UI/docs warn admin to keep an offline copy; restore checks report
  backup health.

## C010: Backup Boundary

- Status: Open
- Risk: High
- Related tasks: T402
- Caveat: Backing up temp/staging/incomplete uploads can preserve partial or
  corrupt state; excluding too much can make restore incomplete.
- Mitigation: Back up durable state only: DB dumps, originals, derivatives,
  model packs, exports/manifests, and settings.

## C011: Share Metadata Leakage

- Status: Open
- Risk: High
- Related tasks: T401
- Caveat: Private share links can leak GPS/full EXIF, people labels, or
  referrers if pages are not carefully constrained.
- Mitigation: Omit GPS/full EXIF and people labels by default; set
  `Referrer-Policy: no-referrer` and `X-Robots-Tag: noindex`; no third-party
  scripts.

## C012: Model Supply Chain

- Status: Open
- Risk: High
- Related tasks: T501, T502
- Caveat: Model downloads are executable-adjacent supply-chain inputs and model
  license changes can affect distribution.
- Mitigation: Pinned revisions, checksums, license metadata, and golden
  self-tests before activation.

## C013: Embedding Revision Mixing

- Status: Open
- Risk: High
- Related tasks: T501, T502
- Caveat: Embeddings from different model revisions are incompatible.
- Mitigation: Store embeddings by model key/revision and keep old active index
  until reindex completes.

## C014: Manual API Clients Can Drift

- Status: Open
- Risk: Medium
- Related tasks: T105, T202, T302, T401, T502
- Caveat: Web and Android clients are hand-written, so they can drift from
  OpenAPI and backend behavior.
- Mitigation: Contract tests against OpenAPI examples and vertical flow tests.
