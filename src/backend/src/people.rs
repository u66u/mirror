//! Owner-local people review state.
//!
//! Face detection and embedding are ML side effects; this module keeps the
//! review/clustering rules pure so they can be tested without a face model.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;
use uuid::Uuid;

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
#[derive(Debug, Error, PartialEq, Eq)]
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

fn normalize_person_name(name: &str) -> Result<String, PeopleReviewError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(PeopleReviewError::InvalidDisplayName);
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_controls_trusted_identity_state() -> Result<(), PeopleReviewError> {
        let mut state = PeopleReviewState::default();
        let person_id = Uuid::now_v7();
        state.add_person(person_id, 1, None, PersonReviewStatus::Unreviewed);

        assert!(!is_user_trusted_identity(&state.people[&person_id]));
        state.rename_person(person_id, " Ada ")?;

        let person = &state.people[&person_id];
        assert_eq!(person.display_name.as_deref(), Some("Ada"));
        assert!(is_user_trusted_identity(person));
        Ok(())
    }

    #[test]
    fn merge_split_hide_and_unassign_preserve_face_assignments() -> Result<(), PeopleReviewError> {
        let mut state = PeopleReviewState::default();
        let first_person = Uuid::now_v7();
        let second_person = Uuid::now_v7();
        let split_person = Uuid::now_v7();
        let first_face = Uuid::now_v7();
        let second_face = Uuid::now_v7();
        state.add_person(
            first_person,
            1,
            Some("Ada".to_owned()),
            PersonReviewStatus::Reviewed,
        );
        state.add_person(second_person, 1, None, PersonReviewStatus::Unreviewed);
        state.add_face(first_face, 1);
        state.add_face(second_face, 1);
        state.assign_face(first_person, first_face)?;
        state.assign_face(second_person, second_face)?;

        state.merge_people(first_person, second_person)?;
        assert!(!state.people.contains_key(&second_person));
        assert_eq!(state.faces[&second_face].person_id, Some(first_person));

        state.split_faces(first_person, split_person, &BTreeSet::from([second_face]))?;
        assert_eq!(state.faces[&second_face].person_id, Some(split_person));
        assert_eq!(
            state.people[&split_person].review_status,
            PersonReviewStatus::Unreviewed
        );

        state.hide_person(split_person)?;
        assert!(state.faces[&second_face].hidden);

        state.unassign_faces(&BTreeSet::from([second_face]))?;
        assert_eq!(state.faces[&second_face].person_id, None);
        assert!(!state.faces[&second_face].hidden);
        Ok(())
    }

    #[test]
    fn assignments_cannot_cross_owner_boundaries() {
        let mut state = PeopleReviewState::default();
        let person_id = Uuid::now_v7();
        let face_id = Uuid::now_v7();
        state.add_person(person_id, 1, None, PersonReviewStatus::Unreviewed);
        state.add_face(face_id, 2);

        assert_eq!(
            state.assign_face(person_id, face_id),
            Err(PeopleReviewError::OwnerMismatch)
        );
    }
}
