1. [deferred by user] This is dumb, let's implement automatic error message conversion with thiserror or whatever else. This applies to all errors everywhere, unless we have a valid reason not to. Same for fmt::Display for errors, unless auto derives for them lost information
fn handler_error_message(error: &WorkerError) -> String {
    match error {
        WorkerError::Media(error) => error.to_string(),
        WorkerError::Ml(error) => error.to_string(),
        _ => error.to_string(),
    }
}

fn timeout_message(kind: JobKind) -> &'static str {
    match kind {
        JobKind::ExtractMetadata | JobKind::GenerateDerivatives => "media job timed out",
        JobKind::EmbedAsset => "ml job timed out",
    }
}

2. [done] Why are these 2 different functions? Is it just one or no?
pub async fn upsert_asset_embedding_with_dimension(
    pool: &PgPool,
    asset_id: Uuid,
    model_pack_id: Uuid,
    embedding: &ValidatedEmbedding,
    embedding_dimension: i32,
) -> Result<(), SemanticIndexError> {
    let mut tx = pool.begin().await?;
    upsert_asset_embedding_with_dimension_in_tx(
        &mut tx,
        asset_id,
        model_pack_id,
        embedding,
        embedding_dimension,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Transaction-compatible embedding upsert.
pub async fn upsert_asset_embedding_with_dimension_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    asset_id: Uuid,
    model_pack_id: Uuid,
    embedding: &ValidatedEmbedding,
    embedding_dimension: i32,
) -> Result<(), SemanticIndexError> {
    check_dimension(embedding, embedding_dimension)?;
    let vector = Vector::from(embedding.values().to_vec());

    let affected = sqlx::query(
        r#"
        INSERT INTO asset_embeddings (
            asset_id,
            model_pack_id,
            owner_id,
            asset_public_id,
            asset_created_at,
            asset_trashed_at,
            embedding,
            embedding_dimension
        )
        SELECT
            a.id,
            $2,
            a.owner_id,
            a.public_id,
            a.created_at,
            a.trashed_at,
            $3,
            $4
        FROM assets a
        WHERE a.id = $1
        ON CONFLICT (asset_id, model_pack_id)
        DO UPDATE SET
            owner_id = EXCLUDED.owner_id,
            asset_public_id = EXCLUDED.asset_public_id,
            asset_created_at = EXCLUDED.asset_created_at,
            asset_trashed_at = EXCLUDED.asset_trashed_at,
            embedding = EXCLUDED.embedding,
            embedding_dimension = EXCLUDED.embedding_dimension,
            updated_at = now()
        "#,
    )
    .bind(asset_id)
    .bind(model_pack_id)
    .bind(vector)
    .bind(embedding_dimension)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(SemanticIndexError::AssetUnavailable);
    }

    Ok(())
}

3. [done] If you are creating helper functions, such as parse_semantic_distance_metric, search_limit, etc., you need to make sure their presence is justified (they are used more than once or will be used more than once, or they improve readability)
4. [done] Should this be from and to implementation or is this correct?
fn search_item(row: SearchAssetRow) -> AssetTimelineItem {
    let (
        asset_id,
        created_at,
        favorite_at,
        original_blake3,
        media_type,
        size_bytes,
        original_filename,
        thumbnail_format,
        thumbnail_width,
        thumbnail_height,
        preview_format,
        preview_width,
        preview_height,
    ) = row;

    AssetTimelineItem {
        asset_id,
        created_at,
        favorite_at,
        original_blake3,
        media_type,
        size_bytes,
        original_filename,
        thumbnail: derivative_view(thumbnail_format, thumbnail_width, thumbnail_height),
        preview: derivative_view(preview_format, preview_width, preview_height),
    }
}

fn derivative_view(
    format: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
) -> Option<AssetDerivativeView> {
    Some(AssetDerivativeView {
        format: format?,
        width: width?,
        height: height?,
    })
}
same for asset_derivative_view, etc

5. [done] Do we properly handle errors when upload size exceeds limit or when image embedding size during search gets exceeded? Should they be handled client side or what should we do with them?
6. [done] The search filters on denormalized asset_embeddings.asset_trashed_at IS NULL, not the live assets.trashed_at value. The upsert snapshots asset_trashed_at into asset_embeddings, and semantic_search filters on that copied field. But trash_asset and restore_asset update only the assets table in the code shown
7. [done] The public /search route still calls filename search only. The route’s query is documented as “Filename query for v1 metadata search,” and the handler calls search::search_assets, not semantic_text_search
8. [done for v1 exact search; ANN static index intentionally deferred] Query not optimal for large libraries unless the database has appropriate pgvector and filter indexes. pgvector performs exact nearest-neighbor search by default; approximate HNSW or IVFFlat indexes trade some recall for speed. With WHERE model_pack_id = $1 AND owner_id = $2 AND asset_trashed_at IS NULL, pgvector’s own docs recommend starting with indexes on filter columns and considering multicolumn indexes. For vector ANN indexing, the best design depends on whether embedding is vector(n) or variable vector. pgvector can index rows with the same dimensions via fixed columns or expression/partial indexes; for mixed model dimensions, a per-model or per-dimension partial index is usually needed
9. [done] Add tests for “trash after indexing,” “restore after trash,” dimension mismatch, wrong model kind, and dot/cosine score ordering.
