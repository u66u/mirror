use std::collections::BTreeSet;

use mirror_backend::{
    jobs::JobKind,
    people::{PeopleReviewError, PeopleReviewState, PersonReviewStatus, is_user_trusted_identity},
    worker::production_job_kinds,
};
use uuid::Uuid;

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

#[test]
fn production_job_kinds_gate_face_indexing() {
    let without_faces = production_job_kinds(false);
    assert!(without_faces.contains(&JobKind::EmbedAsset));
    assert!(!without_faces.contains(&JobKind::IndexFaces));

    let with_faces = production_job_kinds(true);
    assert!(with_faces.contains(&JobKind::EmbedAsset));
    assert!(with_faces.contains(&JobKind::IndexFaces));
}
