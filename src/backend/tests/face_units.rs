use mirror_backend::face::test_support::{
    raw_embedding_postprocess_batch_for_test, raw_embedding_preprocess_batch_for_test,
    scrfd_anchors_per_location_for_test, validate_embedding_batch_output_for_test,
    validate_scrfd_head_shape_for_test,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn scrfd_head_shape_requires_standard_two_anchors_per_location() {
    let expected = 80 * 80 * scrfd_anchors_per_location_for_test();

    assert!(validate_scrfd_head_shape_for_test(8, expected, expected, 640, 640).is_ok());
    assert!(validate_scrfd_head_shape_for_test(8, 80 * 80, 80 * 80, 640, 640).is_err());
    assert!(
        validate_scrfd_head_shape_for_test(8, expected * 3 / 2, expected * 3 / 2, 640, 640)
            .is_err()
    );
}

#[test]
fn face_chip_preprocessing_builds_one_contiguous_batch_tensor() -> TestResult {
    let (shape, values) = raw_embedding_preprocess_batch_for_test(&[
        [[1, 2, 3], [4, 5, 6]],
        [[7, 8, 9], [10, 11, 12]],
    ])?;

    assert_eq!(shape, vec![2, 3, 1, 2]);
    assert_eq!(values.len(), 12);
    assert_eq!(
        &values[..6],
        &[
            1.0 / 255.0,
            4.0 / 255.0,
            2.0 / 255.0,
            5.0 / 255.0,
            3.0 / 255.0,
            6.0 / 255.0,
        ]
    );
    assert_eq!(
        &values[6..],
        &[
            7.0 / 255.0,
            10.0 / 255.0,
            8.0 / 255.0,
            11.0 / 255.0,
            9.0 / 255.0,
            12.0 / 255.0,
        ]
    );
    Ok(())
}

#[test]
fn face_embedding_batch_output_is_split_and_normalized_per_face() -> TestResult {
    let embeddings =
        raw_embedding_postprocess_batch_for_test(vec![2, 2], vec![3.0, 4.0, 0.0, 2.0], 2, 2)?;

    assert_eq!(embeddings.len(), 2);
    assert!((embeddings[0][0] - 0.6).abs() < 1e-6);
    assert!((embeddings[0][1] - 0.8).abs() < 1e-6);
    assert_eq!(embeddings[1], [0.0, 1.0]);
    Ok(())
}

#[test]
fn face_embedding_batch_output_rejects_aliased_batch_shape() {
    assert!(validate_embedding_batch_output_for_test(vec![1, 4], vec![0.0; 4], 2, 2).is_err());
}
