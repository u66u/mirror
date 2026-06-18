# pgvector ANN Notes

- V1 uses exact vector ordering only.
- Bind vectors through `pgvector::Vector`; do not build vector literals by hand.
- Add HNSW before IVFFlat for large collections unless build time or memory is
  measured as the limiting factor.
- HNSW operator class must match query operator:
  - Cosine `<=>`: `vector_cosine_ops`
  - L2 `<->`: `vector_l2_ops`
  - Inner product `<#>`: `vector_ip_ops`
- Do not expose pgvector `<#>` raw rank as a distance. Mirror returns a score
  where higher is better.
- Filtered ANN queries can return too few rows because filters are applied
  after the ANN scan. Before enabling ANN by default, add recall fixtures that
  include owner/trash/model-pack filters and tune iterative scans.
- If exact search becomes too slow, first try one partial HNSW index per active
  semantic model pack and dimension. Denormalize owner/trash state only if
  measured filtered recall or latency still demands a maintained projection.
- `asset_embeddings.embedding` is plain `vector` so different model dimensions
  can coexist. Any future ANN index may need an expression cast such as
  `embedding::vector(768)` plus a partial `model_pack_id` predicate.
- Reindex/bulk indexing should avoid per-row model metadata fetches and should
  not maintain HNSW row-by-row if rebuild or batch strategy is materially
  faster. Measure before adding an index manager.
