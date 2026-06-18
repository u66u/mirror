# Mirror V2 Backend Features

These backend features are intentionally outside V1. Do not treat them as V1
blockers unless the product scope changes.

## Edge Abuse Prefilter

- Add a process-local rate limiter such as `governor` only when Mirror is
  commonly exposed to noisy public networks or measured Postgres limiter writes
  become a bottleneck.
- Keep Postgres-backed route quotas as the source of truth for sensitive flows;
  any process-local limiter is a cheap prefilter and may reset on restart.
- The prefilter must run after trusted-proxy client IP resolution so forwarded
  headers cannot be spoofed from untrusted peers.

## Public Share Original Downloads

- Add original-byte downloads for public share links only behind an explicit
  per-share policy flag.
- Strip or clearly gate GPS/full EXIF exposure, support range requests, and
  audit downloads before enabling this route.
- Preserve V1 default behavior: public shares expose privacy-filtered metadata
  and derivatives, not full originals.

## Face Recognition Runtime

- Add the real face detection/alignment/embedding runtime after the model-pack
  contract defines detection outputs, crop/alignment inputs, embedding tensor
  names, thresholds, and fixture expectations.
- Keep face data owner-local and disabled by default.
- Reuse the existing V1 tables and review functions for people clusters,
  merge/split/hide/rename, and unassigned faces.
