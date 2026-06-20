use super::{
    FaceDetectionModelConfig, FaceEmbeddingModelConfig, ImagePreprocessConfig,
    ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest, OnnxModelPackConfig,
};

pub(super) fn opencv_yunet_detection() -> ModelPackManifest {
    let model_path = "models/face_detection_yunet_2023mar.onnx".to_owned();
    ModelPackManifest {
        kind: "face_detection".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "opencv-yunet".to_owned(),
        model_revision: "2023mar".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 1,
        distance_metric: "cosine".to_owned(),
        onnx: onnx_config(&model_path),
        image_preprocess: ImagePreprocessConfig {
            width: 320,
            height: 320,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.0; 3],
            std: [1.0 / 255.0; 3],
        },
        face_detection: Some(FaceDetectionModelConfig {
            adapter: "yunet_opencv_compat".to_owned(),
            model_path: model_path.clone(),
            input_name: "input".to_owned(),
            boxes_output_name: "unused_boxes".to_owned(),
            scores_output_name: "unused_scores".to_owned(),
            landmarks_output_name: None,
            output_names: Vec::new(),
            box_coordinate_space: "pixel".to_owned(),
            box_format: "xywh".to_owned(),
            score_threshold: 0.3,
            min_face_size_ratio: 0.15,
            nms_threshold: 0.3,
            max_faces: 8,
        }),
        face_embedding: None,
        files: files(&[&model_path, "self-tests/face.jpg"]),
        self_tests: preset_self_tests(),
    }
}

pub(super) fn opencv_sface_embedding() -> ModelPackManifest {
    let model_path = "models/face_recognition_sface_2021dec.onnx".to_owned();
    ModelPackManifest {
        kind: "face_embedding".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "opencv-sface".to_owned(),
        model_revision: "2021dec".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 128,
        distance_metric: "cosine".to_owned(),
        onnx: onnx_config(&model_path),
        image_preprocess: ImagePreprocessConfig {
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.0; 3],
            std: [1.0 / 255.0; 3],
        },
        face_detection: None,
        face_embedding: Some(FaceEmbeddingModelConfig {
            adapter: "sface_opencv_compat".to_owned(),
            model_path: model_path.clone(),
            input_name: "data".to_owned(),
            output_name: "fc1".to_owned(),
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            alignment: "five_point".to_owned(),
            mean: [0.0; 3],
            std: [1.0 / 255.0; 3],
            match_threshold: 0.363,
            l2_normalize_output: true,
        }),
        files: files(&[&model_path, "self-tests/aligned-face.jpg"]),
        self_tests: preset_self_tests(),
    }
}

pub(super) fn insightface_scrfd_arcface() -> ModelPackManifest {
    let detector_path = "models/det_10g.onnx".to_owned();
    let embedder_path = "models/w600k_r50.onnx".to_owned();
    ModelPackManifest {
        kind: "face_identity".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "insightface-buffalo-l".to_owned(),
        model_revision: "scrfd10g-w600k-r50".to_owned(),
        license: "model-license-required".to_owned(),
        embedding_dimension: 512,
        distance_metric: "cosine".to_owned(),
        onnx: onnx_config(&detector_path),
        image_preprocess: ImagePreprocessConfig {
            width: 640,
            height: 640,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.5; 3],
            std: [128.0 / 255.0; 3],
        },
        face_detection: Some(FaceDetectionModelConfig {
            adapter: "scrfd".to_owned(),
            model_path: detector_path.clone(),
            input_name: "input.1".to_owned(),
            boxes_output_name: "unused_boxes".to_owned(),
            scores_output_name: "unused_scores".to_owned(),
            landmarks_output_name: None,
            output_names: [
                "448", "471", "494", "451", "474", "497", "454", "477", "500",
            ]
            .map(str::to_owned)
            .to_vec(),
            box_coordinate_space: "pixel".to_owned(),
            box_format: "xyxy".to_owned(),
            score_threshold: 0.5,
            min_face_size_ratio: 0.15,
            nms_threshold: 0.4,
            max_faces: 16,
        }),
        face_embedding: Some(FaceEmbeddingModelConfig {
            adapter: "arcface".to_owned(),
            model_path: embedder_path.clone(),
            input_name: "input.1".to_owned(),
            output_name: "683".to_owned(),
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            alignment: "five_point".to_owned(),
            mean: [0.5; 3],
            std: [0.5; 3],
            match_threshold: 0.55,
            l2_normalize_output: true,
        }),
        files: files(&[&detector_path, &embedder_path, "self-tests/face.jpg"]),
        self_tests: preset_self_tests(),
    }
}

fn onnx_config(model_path: &str) -> OnnxModelPackConfig {
    OnnxModelPackConfig {
        image_model_path: model_path.to_owned(),
        text_model_path: model_path.to_owned(),
        tokenizer_path: "self-tests/face.jpg".to_owned(),
        image_input_name: "unused_image".to_owned(),
        image_output_name: "unused_image_output".to_owned(),
        text_input_ids_name: "unused_input_ids".to_owned(),
        text_attention_mask_name: "unused_attention_mask".to_owned(),
        text_output_name: "unused_text_output".to_owned(),
    }
}

fn files(paths: &[&str]) -> Vec<ModelPackFileManifest> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| ModelPackFileManifest {
            path: (*path).to_owned(),
            sha256: format!("{index:064x}"),
            size_bytes: 1,
        })
        .collect()
}

fn preset_self_tests() -> Vec<ModelPackSelfTestManifest> {
    vec![ModelPackSelfTestManifest {
        name: "face_fixture".to_owned(),
        input_path: "self-tests/face.jpg".to_owned(),
        expected_output_sha256: "0".repeat(64),
    }]
}
