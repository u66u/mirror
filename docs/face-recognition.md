# Face Recognition Backend Notes

Mirror model-pack manifests are JSON only. The canonical install file is
`manifest.json`; the API, validator, schema generator, and database all use the
same `ModelPackManifest` shape.

## Presets

Built-in preset templates are printed by the maintenance binary:

```sh
maintenance --model-pack-presets
maintenance --model-pack-preset opencv_yunet_detection_2023mar
maintenance --model-pack-preset opencv_sface_embedding_2021dec
maintenance --model-pack-preset insightface_buffalo_l_scrfd_arcface
maintenance --materialize-model-pack-preset insightface_buffalo_l_scrfd_arcface ./data
```

Preset JSON uses placeholder `files[].sha256`, `files[].size_bytes`, and
`self_tests[].expected_output_sha256` values. Operators must replace them with
the exact local model and self-test fixture values before validation/install.
The materializer fills `files[].sha256` and `files[].size_bytes` from a local
directory; runtime self-test output hashes still require an actual self-test
run.

## Model Identity

Face embeddings are model-pack scoped. Do not cluster or compare embeddings
across different embedding model packs, adapter kinds, dimensions, distance
metrics, normalization settings, or manifest revisions. Reindex assets after
changing either the detector or embedder.

## Default Thresholds

OpenCV YuNet detection preset:

- `score_threshold = 0.3`
- `nms_threshold = 0.3`
- `min_face_size_ratio = 0.15`
- `max_faces = 8`

OpenCV SFace embedding preset:

- `match_threshold = 0.363`
- `distance_metric = "cosine"`
- `l2_normalize_output = true`

InsightFace SCRFD + ArcFace preset:

- `score_threshold = 0.5`
- `nms_threshold = 0.4`
- `min_face_size_ratio = 0.15`
- `match_threshold = 0.55`
- `embedding_dimension = 512`
- `distance_metric = "cosine"`
- `l2_normalize_output = true`

These are starting defaults, not universal identity guarantees. Operators should
calibrate thresholds against their chosen model files and fixture set before
trusting automatic people clustering.

## Self-Tests

Face detection packs hash deterministic detector output values: quality, box,
and landmarks when present. Face embedding packs hash embedding vectors from an
already-aligned self-test chip. Combined face identity packs hash detected
indexed-face values plus embeddings.
