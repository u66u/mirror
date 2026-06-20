//! Owner-local people review state.
//!
//! Face detection and embedding are ML side effects; this module keeps the
//! review/clustering rules pure so they can be tested without a face model.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sqlx::PgPool;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::storage::{StorageKey, StorageKeyError};

const DEFAULT_FACE_LIST_LIMIT: i64 = 100;
const MAX_FACE_LIST_LIMIT: i64 = 500;

/// Review status for a person cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonReviewStatus {
    /// Machine-created cluster that the owner has not trusted yet.
    Unreviewed,
    /// Owner reviewed or named this cluster.
    Reviewed,
    /// Owner hid this cluster.
    Hidden,
}

/// Person cluster visible only to one owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonCluster {
    /// Cluster ID.
    pub id: Uuid,
    /// Owner ID.
    pub owner_id: i16,
    /// Owner-supplied display name.
    pub display_name: Option<String>,
    /// Review status.
    pub review_status: PersonReviewStatus,
    /// Assigned face IDs.
    pub face_ids: BTreeSet<Uuid>,
}

/// One detected face review assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceReview {
    /// Face occurrence ID.
    pub id: Uuid,
    /// Owner ID.
    pub owner_id: i16,
    /// Assigned person, if any.
    pub person_id: Option<Uuid>,
    /// Hidden faces are excluded from normal people albums.
    pub hidden: bool,
}

/// In-memory review state for pure people operations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PeopleReviewState {
    /// People by ID.
    pub people: BTreeMap<Uuid, PersonCluster>,
    /// Faces by ID.
    pub faces: BTreeMap<Uuid, FaceReview>,
}

/// People review invariant failure.
#[derive(Debug, Error)]
pub enum PeopleReviewError {
    /// Person ID is unknown.
    #[error("person not found")]
    PersonNotFound,
    /// Face ID is unknown.
    #[error("face not found")]
    FaceNotFound,
    /// Operation tried to cross owner boundaries.
    #[error("people operation crosses owner boundary")]
    OwnerMismatch,
    /// Person display name is empty or too long.
    #[error("person display name is invalid")]
    InvalidDisplayName,
    /// Database failed.
    #[error("people database error")]
    Database(#[from] sqlx::Error),
    /// Stored face chip key is invalid.
    #[error("people storage key error")]
    StorageKey(#[from] StorageKeyError),
}

impl PartialEq for PeopleReviewError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::PersonNotFound, Self::PersonNotFound)
                | (Self::FaceNotFound, Self::FaceNotFound)
                | (Self::OwnerMismatch, Self::OwnerMismatch)
                | (Self::InvalidDisplayName, Self::InvalidDisplayName)
                | (Self::Database(_), Self::Database(_))
                | (Self::StorageKey(_), Self::StorageKey(_))
        )
    }
}

impl Eq for PeopleReviewError {}

/// Owner-local people album summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonSummary {
    /// Person cluster ID.
    pub person_id: Uuid,
    /// Owner-supplied display name.
    pub display_name: Option<String>,
    /// Review state.
    pub review_status: String,
    /// Number of visible assigned faces.
    pub face_count: i64,
}

/// Public bounding-box view for a detected face.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FaceBounds {
    /// Left coordinate in `0.0..=1.0`.
    pub left: f32,
    /// Top coordinate in `0.0..=1.0`.
    pub top: f32,
    /// Width in `0.0..=1.0`.
    pub width: f32,
    /// Height in `0.0..=1.0`.
    pub height: f32,
}

/// One face occurrence in a people album or review queue.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FaceAlbumItem {
    /// Face occurrence ID used by review mutations.
    pub face_id: Uuid,
    /// Public asset ID containing the face.
    pub asset_id: Uuid,
    /// Asset creation time for album ordering.
    pub asset_created_at: OffsetDateTime,
    /// Original media type.
    pub media_type: String,
    /// Normalized face bounds within the asset.
    pub bbox: FaceBounds,
    /// Detector confidence if available.
    pub quality: Option<f32>,
    /// Face review state.
    pub review_state: String,
    /// Whether a generated face chip is available.
    pub chip_available: bool,
}

#[derive(Debug)]
struct FaceAlbumRow {
    face_id: Uuid,
    asset_public_id: Uuid,
    asset_created_at: OffsetDateTime,
    media_type: String,
    bbox_left: f32,
    bbox_top: f32,
    bbox_width: f32,
    bbox_height: f32,
    quality: Option<f32>,
    review_state: String,
    chip_available: bool,
}

impl From<FaceAlbumRow> for FaceAlbumItem {
    fn from(row: FaceAlbumRow) -> Self {
        Self {
            face_id: row.face_id,
            asset_id: row.asset_public_id,
            asset_created_at: row.asset_created_at,
            media_type: row.media_type,
            bbox: FaceBounds {
                left: row.bbox_left,
                top: row.bbox_top,
                width: row.bbox_width,
                height: row.bbox_height,
            },
            quality: row.quality,
            review_state: row.review_state,
            chip_available: row.chip_available,
        }
    }
}

/// Stored face chip object metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceChipObject {
    /// Storage key containing the generated chip.
    pub storage_key: StorageKey,
    /// HTTP media type for the encoded chip.
    pub media_type: &'static str,
}

impl PeopleReviewState {
    /// Inserts or replaces a face for pure tests/imports.
    pub fn add_face(&mut self, id: Uuid, owner_id: i16) {
        self.faces.insert(
            id,
            FaceReview {
                id,
                owner_id,
                person_id: None,
                hidden: false,
            },
        );
    }

    /// Inserts or replaces a person cluster for pure tests/imports.
    pub fn add_person(
        &mut self,
        id: Uuid,
        owner_id: i16,
        display_name: Option<String>,
        review_status: PersonReviewStatus,
    ) {
        self.people.insert(
            id,
            PersonCluster {
                id,
                owner_id,
                display_name,
                review_status,
                face_ids: BTreeSet::new(),
            },
        );
    }

    /// Assigns one face to one person.
    pub fn assign_face(&mut self, person_id: Uuid, face_id: Uuid) -> Result<(), PeopleReviewError> {
        let owner_id = self
            .people
            .get(&person_id)
            .ok_or(PeopleReviewError::PersonNotFound)?
            .owner_id;
        let face = self
            .faces
            .get_mut(&face_id)
            .ok_or(PeopleReviewError::FaceNotFound)?;
        if face.owner_id != owner_id {
            return Err(PeopleReviewError::OwnerMismatch);
        }
        if let Some(old_person_id) = face.person_id
            && let Some(old_person) = self.people.get_mut(&old_person_id)
        {
            old_person.face_ids.remove(&face_id);
        }
        face.person_id = Some(person_id);
        face.hidden = false;
        self.people
            .get_mut(&person_id)
            .ok_or(PeopleReviewError::PersonNotFound)?
            .face_ids
            .insert(face_id);
        Ok(())
    }

    /// Renames and trusts a person cluster.
    pub fn rename_person(&mut self, person_id: Uuid, name: &str) -> Result<(), PeopleReviewError> {
        let name = normalize_person_name(name)?;
        let person = self
            .people
            .get_mut(&person_id)
            .ok_or(PeopleReviewError::PersonNotFound)?;
        person.display_name = Some(name);
        person.review_status = PersonReviewStatus::Reviewed;
        Ok(())
    }

    /// Hides a person and all currently assigned faces.
    pub fn hide_person(&mut self, person_id: Uuid) -> Result<(), PeopleReviewError> {
        let face_ids = {
            let person = self
                .people
                .get_mut(&person_id)
                .ok_or(PeopleReviewError::PersonNotFound)?;
            person.review_status = PersonReviewStatus::Hidden;
            person.face_ids.iter().copied().collect::<Vec<_>>()
        };
        for face_id in face_ids {
            if let Some(face) = self.faces.get_mut(&face_id) {
                face.hidden = true;
            }
        }
        Ok(())
    }

    /// Merges source into target and removes the source cluster.
    pub fn merge_people(
        &mut self,
        target_id: Uuid,
        source_id: Uuid,
    ) -> Result<(), PeopleReviewError> {
        let target_owner = self
            .people
            .get(&target_id)
            .ok_or(PeopleReviewError::PersonNotFound)?
            .owner_id;
        let source = self
            .people
            .remove(&source_id)
            .ok_or(PeopleReviewError::PersonNotFound)?;
        if source.owner_id != target_owner {
            self.people.insert(source_id, source);
            return Err(PeopleReviewError::OwnerMismatch);
        }
        let target = self
            .people
            .get_mut(&target_id)
            .ok_or(PeopleReviewError::PersonNotFound)?;
        if target.display_name.is_none() {
            target.display_name = source.display_name;
        }
        if source.review_status == PersonReviewStatus::Reviewed {
            target.review_status = PersonReviewStatus::Reviewed;
        }
        for face_id in source.face_ids {
            if let Some(face) = self.faces.get_mut(&face_id) {
                face.person_id = Some(target_id);
            }
            target.face_ids.insert(face_id);
        }
        Ok(())
    }

    /// Moves selected faces from an existing person into a new unreviewed person.
    pub fn split_faces(
        &mut self,
        source_id: Uuid,
        new_person_id: Uuid,
        face_ids: &BTreeSet<Uuid>,
    ) -> Result<(), PeopleReviewError> {
        let owner_id = self
            .people
            .get(&source_id)
            .ok_or(PeopleReviewError::PersonNotFound)?
            .owner_id;
        self.add_person(
            new_person_id,
            owner_id,
            None,
            PersonReviewStatus::Unreviewed,
        );
        for face_id in face_ids {
            if !self
                .people
                .get(&source_id)
                .ok_or(PeopleReviewError::PersonNotFound)?
                .face_ids
                .contains(face_id)
            {
                return Err(PeopleReviewError::FaceNotFound);
            }
            self.assign_face(new_person_id, *face_id)?;
        }
        Ok(())
    }

    /// Removes face assignments and returns faces to the unassigned pool.
    pub fn unassign_faces(&mut self, face_ids: &BTreeSet<Uuid>) -> Result<(), PeopleReviewError> {
        for face_id in face_ids {
            let person_id = self
                .faces
                .get(face_id)
                .ok_or(PeopleReviewError::FaceNotFound)?
                .person_id;
            if let Some(person_id) = person_id
                && let Some(person) = self.people.get_mut(&person_id)
            {
                person.face_ids.remove(face_id);
            }
            let face = self
                .faces
                .get_mut(face_id)
                .ok_or(PeopleReviewError::FaceNotFound)?;
            face.person_id = None;
            face.hidden = false;
        }
        Ok(())
    }
}

/// A cluster is user-trusted identity only after owner review and naming.
#[must_use]
pub fn is_user_trusted_identity(person: &PersonCluster) -> bool {
    person.review_status == PersonReviewStatus::Reviewed && person.display_name.is_some()
}

/// Lists owner-local people clusters.
pub async fn list_people(
    pool: &PgPool,
    owner_id: i16,
) -> Result<Vec<PersonSummary>, PeopleReviewError> {
    sqlx::query_as!(
        PersonSummary,
        r#"
        SELECT
            p.id AS person_id,
            p.display_name,
            p.review_status,
            count(pf.face_occurrence_id) AS "face_count!"
        FROM people p
        LEFT JOIN person_faces pf
          ON pf.person_id = p.id
         AND pf.owner_id = p.owner_id
         AND pf.review_state = 'assigned'
        WHERE p.owner_id = $1
          AND p.review_status <> 'hidden'
        GROUP BY p.id, p.display_name, p.review_status
        ORDER BY
            p.review_status ASC,
            p.display_name ASC NULLS LAST,
            count(pf.face_occurrence_id) DESC,
            p.created_at DESC
        "#,
        owner_id
    )
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

/// Lists visible assigned faces for one owner-local person cluster.
pub async fn list_person_faces(
    pool: &PgPool,
    owner_id: i16,
    person_id: Uuid,
    limit: Option<i64>,
) -> Result<Vec<FaceAlbumItem>, PeopleReviewError> {
    ensure_visible_person(pool, owner_id, person_id).await?;
    let rows = sqlx::query_as!(
        FaceAlbumRow,
        r#"
        SELECT
            fo.id AS face_id,
            a.public_id AS asset_public_id,
            a.created_at AS asset_created_at,
            o.media_type,
            fo.bbox_left,
            fo.bbox_top,
            fo.bbox_width,
            fo.bbox_height,
            fo.quality,
            fo.review_state,
            fo.chip_storage_key IS NOT NULL AS "chip_available!"
        FROM person_faces pf
        JOIN face_occurrences fo
          ON fo.id = pf.face_occurrence_id
         AND fo.owner_id = pf.owner_id
        JOIN assets a
          ON a.id = fo.asset_id
         AND a.owner_id = fo.owner_id
        JOIN originals o
          ON o.id = a.original_id
        WHERE pf.owner_id = $1
          AND pf.person_id = $2
          AND pf.review_state = 'assigned'
          AND fo.review_state = 'assigned'
          AND a.trashed_at IS NULL
        ORDER BY a.created_at DESC, fo.created_at DESC, fo.id DESC
        LIMIT $3
        "#,
        owner_id,
        person_id,
        face_list_limit(limit)
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Lists owner-local faces that are not assigned to a visible person.
pub async fn list_unassigned_faces(
    pool: &PgPool,
    owner_id: i16,
    limit: Option<i64>,
) -> Result<Vec<FaceAlbumItem>, PeopleReviewError> {
    let rows = sqlx::query_as!(
        FaceAlbumRow,
        r#"
        SELECT
            fo.id AS face_id,
            a.public_id AS asset_public_id,
            a.created_at AS asset_created_at,
            o.media_type,
            fo.bbox_left,
            fo.bbox_top,
            fo.bbox_width,
            fo.bbox_height,
            fo.quality,
            fo.review_state,
            fo.chip_storage_key IS NOT NULL AS "chip_available!"
        FROM face_occurrences fo
        JOIN assets a
          ON a.id = fo.asset_id
         AND a.owner_id = fo.owner_id
        JOIN originals o
          ON o.id = a.original_id
        LEFT JOIN person_faces pf
          ON pf.face_occurrence_id = fo.id
         AND pf.owner_id = fo.owner_id
        WHERE fo.owner_id = $1
          AND fo.review_state = 'unassigned'
          AND pf.face_occurrence_id IS NULL
          AND a.trashed_at IS NULL
        ORDER BY a.created_at DESC, fo.created_at DESC, fo.id DESC
        LIMIT $2
        "#,
        owner_id,
        face_list_limit(limit)
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// Resolves a generated face chip for an owner-visible face.
pub async fn get_face_chip(
    pool: &PgPool,
    owner_id: i16,
    face_id: Uuid,
) -> Result<FaceChipObject, PeopleReviewError> {
    let row = sqlx::query!(
        r#"
        SELECT
            fo.chip_storage_key AS "chip_storage_key!",
            fo.chip_format AS "chip_format!"
        FROM face_occurrences fo
        JOIN assets a
          ON a.id = fo.asset_id
         AND a.owner_id = fo.owner_id
        WHERE fo.owner_id = $1
          AND fo.id = $2
          AND fo.review_state <> 'hidden'
          AND fo.chip_storage_key IS NOT NULL
          AND a.trashed_at IS NULL
        "#,
        owner_id,
        face_id
    )
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Err(PeopleReviewError::FaceNotFound);
    };
    let media_type = match row.chip_format.as_str() {
        "webp" => "image/webp",
        _ => return Err(StorageKeyError::UnsafePath.into()),
    };
    Ok(FaceChipObject {
        storage_key: StorageKey::new(row.chip_storage_key)?,
        media_type,
    })
}

async fn ensure_visible_person(
    pool: &PgPool,
    owner_id: i16,
    person_id: Uuid,
) -> Result<(), PeopleReviewError> {
    let exists = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM people
            WHERE id = $1
              AND owner_id = $2
              AND review_status <> 'hidden'
        ) AS "exists!"
        "#,
        person_id,
        owner_id
    )
    .fetch_one(pool)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(PeopleReviewError::PersonNotFound)
    }
}

fn face_list_limit(limit: Option<i64>) -> i64 {
    limit
        .unwrap_or(DEFAULT_FACE_LIST_LIMIT)
        .clamp(1, MAX_FACE_LIST_LIMIT)
}

/// Renames and trusts a persisted person cluster.
pub async fn rename_person(
    pool: &PgPool,
    owner_id: i16,
    person_id: Uuid,
    name: &str,
) -> Result<(), PeopleReviewError> {
    let name = normalize_person_name(name)?;
    let result = sqlx::query!(
        r#"
        UPDATE people
        SET display_name = $1, review_status = 'reviewed', updated_at = now()
        WHERE id = $2 AND owner_id = $3 AND review_status <> 'hidden'
        "#,
        name,
        person_id,
        owner_id
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PeopleReviewError::PersonNotFound);
    }
    Ok(())
}

/// Hides a persisted person cluster and its assigned faces.
pub async fn hide_person(
    pool: &PgPool,
    owner_id: i16,
    person_id: Uuid,
) -> Result<(), PeopleReviewError> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        r#"
        UPDATE people
        SET review_status = 'hidden', updated_at = now()
        WHERE id = $1 AND owner_id = $2
        "#,
        person_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PeopleReviewError::PersonNotFound);
    }
    sqlx::query!(
        r#"
        UPDATE person_faces
        SET review_state = 'hidden', updated_at = now()
        WHERE person_id = $1 AND owner_id = $2
        "#,
        person_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"
        UPDATE face_occurrences fo
        SET review_state = 'hidden', updated_at = now()
        FROM person_faces pf
        WHERE pf.face_occurrence_id = fo.id
          AND pf.owner_id = fo.owner_id
          AND pf.person_id = $1
          AND pf.owner_id = $2
        "#,
        person_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Merges a source person into a target person.
pub async fn merge_people(
    pool: &PgPool,
    owner_id: i16,
    target_id: Uuid,
    source_id: Uuid,
) -> Result<(), PeopleReviewError> {
    if target_id == source_id {
        return Err(PeopleReviewError::PersonNotFound);
    }
    let mut tx = pool.begin().await?;
    let rows = sqlx::query!(
        r#"
        SELECT id, display_name, review_status
        FROM people
        WHERE owner_id = $1 AND id = ANY($2)
        FOR UPDATE
        "#,
        owner_id,
        &[target_id, source_id]
    )
    .fetch_all(&mut *tx)
    .await?;
    if rows.len() != 2 {
        return Err(PeopleReviewError::PersonNotFound);
    }
    let source_name = rows
        .iter()
        .find(|row| row.id == source_id)
        .and_then(|row| row.display_name.clone());
    let source_reviewed = rows
        .iter()
        .any(|row| row.id == source_id && row.review_status == "reviewed");
    sqlx::query!(
        r#"
        UPDATE person_faces
        SET person_id = $1, updated_at = now()
        WHERE person_id = $2 AND owner_id = $3
        "#,
        target_id,
        source_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"
        UPDATE people
        SET
            display_name = COALESCE(display_name, $1),
            review_status = CASE WHEN $2 THEN 'reviewed' ELSE review_status END,
            updated_at = now()
        WHERE id = $3 AND owner_id = $4
        "#,
        source_name,
        source_reviewed,
        target_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "DELETE FROM people WHERE id = $1 AND owner_id = $2",
        source_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Splits selected faces from an existing person into a new unreviewed cluster.
pub async fn split_faces(
    pool: &PgPool,
    owner_id: i16,
    source_id: Uuid,
    face_ids: &[Uuid],
) -> Result<Uuid, PeopleReviewError> {
    if face_ids.is_empty() {
        return Err(PeopleReviewError::FaceNotFound);
    }
    let new_person_id = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    sqlx::query!(
        r#"
        INSERT INTO people (id, owner_id, display_name, review_status)
        VALUES ($1, $2, NULL, 'unreviewed')
        "#,
        new_person_id,
        owner_id
    )
    .execute(&mut *tx)
    .await?;
    let result = sqlx::query!(
        r#"
        UPDATE person_faces
        SET person_id = $1, updated_at = now()
        WHERE person_id = $2
          AND owner_id = $3
          AND face_occurrence_id = ANY($4)
        "#,
        new_person_id,
        source_id,
        owner_id,
        face_ids
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != face_ids.len() as u64 {
        return Err(PeopleReviewError::FaceNotFound);
    }
    tx.commit().await?;
    Ok(new_person_id)
}

/// Removes persisted face assignments.
pub async fn unassign_faces(
    pool: &PgPool,
    owner_id: i16,
    face_ids: &[Uuid],
) -> Result<(), PeopleReviewError> {
    if face_ids.is_empty() {
        return Err(PeopleReviewError::FaceNotFound);
    }
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        r#"
        DELETE FROM person_faces
        WHERE owner_id = $1
          AND face_occurrence_id = ANY($2)
        "#,
        owner_id,
        face_ids
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != face_ids.len() as u64 {
        return Err(PeopleReviewError::FaceNotFound);
    }
    sqlx::query!(
        r#"
        UPDATE face_occurrences
        SET review_state = 'unassigned', updated_at = now()
        WHERE owner_id = $1
          AND id = ANY($2)
        "#,
        owner_id,
        face_ids
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

fn normalize_person_name(name: &str) -> Result<String, PeopleReviewError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(PeopleReviewError::InvalidDisplayName);
    }
    Ok(name.to_owned())
}
