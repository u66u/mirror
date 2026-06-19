use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use actix_web::{App, cookie::Cookie, http::StatusCode, test as actix_test, web};
use mirror_backend::{
    auth::{SessionCreateInput, SessionCreateOutput, SetupState, create_session},
    config::{Config, MlDevicePreference},
    face::{FaceRuntime, FaceRuntimeRequest, OnnxFaceRuntime, SharedFaceRuntime},
    http,
    jobs::JobKind,
    ml::MlRuntime,
    models::{
        FaceDetectionModelConfig, FaceEmbeddingModelConfig, ImagePreprocessConfig,
        ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest, OnnxModelPackConfig,
    },
    state::AppState,
    storage::ObjectStorage,
    worker::{WorkerHandlers, WorkerPolicy, WorkerStep, run_once},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tempfile::TempDir;
use uuid::Uuid;

mod support;
use support::{FakeImageProcessor, FakeImageTextEmbedder, FakeVideoProcessor};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone)]
struct FaceFixture {
    filename: String,
    label: String,
    media_type: &'static str,
}

#[derive(Debug, Deserialize)]
struct ReferenceFaceOutput {
    filename: String,
    bbox: [f32; 4],
    score: f32,
    embedding: Vec<f32>,
}

#[test]
#[ignore = "requires local data/ YuNet/SFace fixtures and a system ONNX Runtime library"]
fn opencv_yunet_sface_models_cluster_local_people_fixtures() -> TestResult {
    require_system_onnxruntime()?;

    let storage_root = TempDir::new()?;
    let detection_pack_id = Uuid::now_v7();
    let embedding_pack_id = Uuid::now_v7();
    copy_model_fixture(
        storage_root.path(),
        detection_pack_id,
        "models/face_detection_yunet_2023mar.onnx",
        data_path("face_detection_yunet_2023mar.onnx"),
    )?;
    copy_model_fixture(
        storage_root.path(),
        embedding_pack_id,
        "models/face_recognition_sface_2021dec.onnx",
        data_path("face_recognition_sface_2021dec.onnx"),
    )?;

    let detection_manifest = yunet_manifest();
    let embedding_manifest = sface_manifest();
    let runtime = OnnxFaceRuntime::new(
        storage_root.path().to_path_buf(),
        MlDevicePreference::CpuOnly,
    );

    assert_fixture_embeddings_cluster(
        &runtime,
        detection_pack_id,
        &detection_manifest,
        embedding_pack_id,
        &embedding_manifest,
        0.363,
    )
}

#[test]
#[ignore = "requires local data/ YuNet/SFace fixtures, system ONNX Runtime, and Python opencv-contrib-python"]
fn opencv_reference_yunet_sface_matches_backend_outputs() -> TestResult {
    require_system_onnxruntime()?;

    let storage_root = TempDir::new()?;
    let detection_pack_id = Uuid::now_v7();
    let embedding_pack_id = Uuid::now_v7();
    copy_model_fixture(
        storage_root.path(),
        detection_pack_id,
        "models/face_detection_yunet_2023mar.onnx",
        data_path("face_detection_yunet_2023mar.onnx"),
    )?;
    copy_model_fixture(
        storage_root.path(),
        embedding_pack_id,
        "models/face_recognition_sface_2021dec.onnx",
        data_path("face_recognition_sface_2021dec.onnx"),
    )?;

    let fixtures = discover_numbered_face_fixtures()?;
    let runtime = OnnxFaceRuntime::new(
        storage_root.path().to_path_buf(),
        MlDevicePreference::CpuOnly,
    );
    let backend = backend_face_outputs(
        &runtime,
        &fixtures,
        detection_pack_id,
        &yunet_manifest(),
        embedding_pack_id,
        &sface_manifest(),
    )?;
    let reference = run_opencv_yunet_sface_reference(&fixtures)?;
    assert_reference_outputs_match("OpenCV YuNet/SFace", &backend, &reference, 0.95, 0.95)
}

#[test]
#[ignore = "requires local data/ InsightFace SCRFD/ArcFace fixtures and a system ONNX Runtime library"]
fn insightface_scrfd_arcface_models_cluster_local_people_fixtures() -> TestResult {
    require_system_onnxruntime()?;

    let storage_root = TempDir::new()?;
    let detection_pack_id = Uuid::now_v7();
    let embedding_pack_id = Uuid::now_v7();
    copy_model_fixture(
        storage_root.path(),
        detection_pack_id,
        "models/det_10g.onnx",
        data_path("det_10g.onnx"),
    )?;
    copy_model_fixture(
        storage_root.path(),
        embedding_pack_id,
        "models/w600k_r50.onnx",
        data_path("w600k_r50.onnx"),
    )?;

    let detection_manifest = scrfd_manifest();
    let embedding_manifest = arcface_manifest();
    let runtime = OnnxFaceRuntime::new(
        storage_root.path().to_path_buf(),
        MlDevicePreference::CpuOnly,
    );

    assert_fixture_embeddings_cluster(
        &runtime,
        detection_pack_id,
        &detection_manifest,
        embedding_pack_id,
        &embedding_manifest,
        0.55,
    )
}

#[test]
#[ignore = "requires local data/ SCRFD/ArcFace fixtures, system ONNX Runtime, and Python insightface"]
fn insightface_reference_scrfd_arcface_matches_backend_outputs() -> TestResult {
    require_system_onnxruntime()?;

    let storage_root = TempDir::new()?;
    let detection_pack_id = Uuid::now_v7();
    let embedding_pack_id = Uuid::now_v7();
    copy_model_fixture(
        storage_root.path(),
        detection_pack_id,
        "models/det_10g.onnx",
        data_path("det_10g.onnx"),
    )?;
    copy_model_fixture(
        storage_root.path(),
        embedding_pack_id,
        "models/w600k_r50.onnx",
        data_path("w600k_r50.onnx"),
    )?;

    let fixtures = discover_numbered_face_fixtures()?;
    let runtime = OnnxFaceRuntime::new(
        storage_root.path().to_path_buf(),
        MlDevicePreference::CpuOnly,
    );
    let backend = backend_face_outputs(
        &runtime,
        &fixtures,
        detection_pack_id,
        &scrfd_manifest(),
        embedding_pack_id,
        &arcface_manifest(),
    )?;
    let reference = run_insightface_scrfd_arcface_reference(&fixtures)?;
    assert_reference_outputs_match(
        "InsightFace SCRFD/ArcFace",
        &backend,
        &reference,
        0.98,
        0.94,
    )
}

fn assert_fixture_embeddings_cluster(
    runtime: &OnnxFaceRuntime,
    detection_pack_id: Uuid,
    detection_manifest: &ModelPackManifest,
    embedding_pack_id: Uuid,
    embedding_manifest: &ModelPackManifest,
    threshold: f32,
) -> TestResult {
    let fixtures = discover_numbered_face_fixtures()?;
    let mut embeddings = Vec::new();
    for fixture in &fixtures {
        let bytes = fs::read(data_path(&fixture.filename))?;
        let faces = runtime.detect_and_embed(FaceRuntimeRequest {
            bytes: &bytes,
            media_type: fixture.media_type,
            detection_model_pack_id: detection_pack_id,
            detection_manifest,
            embedding_model_pack_id: embedding_pack_id,
            embedding_manifest,
        })?;
        assert!(
            !faces.is_empty(),
            "{} should contain at least one face",
            fixture.filename
        );
        embeddings.push((
            fixture.filename.as_str(),
            fixture.label.as_str(),
            faces[0].embedding.clone(),
        ));
    }

    let mut same_person_pairs = 0;
    let mut different_person_pairs = 0;
    let mut scores = Vec::new();
    for left in 0..embeddings.len() {
        for right in left + 1..embeddings.len() {
            let score = cosine(&embeddings[left].2, &embeddings[right].2);
            scores.push(format!(
                "{}:{} vs {}:{} cosine={score:.6}",
                embeddings[left].0, embeddings[left].1, embeddings[right].0, embeddings[right].1
            ));
            if embeddings[left].1 == embeddings[right].1 {
                assert!(
                    score >= threshold,
                    "{} and {} should match, score {score}\n{}",
                    embeddings[left].0,
                    embeddings[right].0,
                    scores.join("\n")
                );
                same_person_pairs += 1;
            } else {
                assert!(
                    score < threshold,
                    "{} and {} should not match, score {score}\n{}",
                    embeddings[left].0,
                    embeddings[right].0,
                    scores.join("\n")
                );
                different_person_pairs += 1;
            }
        }
    }
    assert!(same_person_pairs > 0);
    assert!(different_person_pairs > 0);
    Ok(())
}

fn backend_face_outputs(
    runtime: &OnnxFaceRuntime,
    fixtures: &[FaceFixture],
    detection_pack_id: Uuid,
    detection_manifest: &ModelPackManifest,
    embedding_pack_id: Uuid,
    embedding_manifest: &ModelPackManifest,
) -> TestResult<Vec<ReferenceFaceOutput>> {
    let mut outputs = Vec::new();
    for fixture in fixtures {
        let bytes = fs::read(data_path(&fixture.filename))?;
        let faces = runtime.detect_and_embed(FaceRuntimeRequest {
            bytes: &bytes,
            media_type: fixture.media_type,
            detection_model_pack_id: detection_pack_id,
            detection_manifest,
            embedding_model_pack_id: embedding_pack_id,
            embedding_manifest,
        })?;
        let face = faces
            .first()
            .ok_or_else(|| format!("{} should contain at least one face", fixture.filename))?;
        let mut embedding = face.embedding.clone();
        normalize_vector_l2(&mut embedding);
        outputs.push(ReferenceFaceOutput {
            filename: fixture.filename.clone(),
            bbox: [
                face.bbox.left,
                face.bbox.top,
                face.bbox.width,
                face.bbox.height,
            ],
            score: face.quality.unwrap_or(0.0),
            embedding,
        });
    }
    Ok(outputs)
}

fn run_opencv_yunet_sface_reference(
    fixtures: &[FaceFixture],
) -> TestResult<Vec<ReferenceFaceOutput>> {
    // The YuNet ONNX file is fixed at 640x640 under ONNX Runtime. This
    // reference keeps OpenCV DNN on the same detector geometry, then maps
    // landmarks back to the original image before SFace alignment like backend.
    run_reference_python(
        fixtures,
        &[
            (
                "MIRROR_FACE_DETECTOR_MODEL",
                data_path("face_detection_yunet_2023mar.onnx"),
            ),
            (
                "MIRROR_FACE_EMBEDDING_MODEL",
                data_path("face_recognition_sface_2021dec.onnx"),
            ),
        ],
        r#"
import cv2, json, os, numpy as np

data_dir = os.environ["MIRROR_FACE_DATA_DIR"]
fixtures = json.loads(os.environ["MIRROR_FACE_FIXTURES"])
detector = cv2.FaceDetectorYN_create(os.environ["MIRROR_FACE_DETECTOR_MODEL"], "", (640, 640), 0.5, 0.3, 5000)
recognizer = cv2.FaceRecognizerSF_create(os.environ["MIRROR_FACE_EMBEDDING_MODEL"], "")
outputs = []
for fixture in fixtures:
    filename = fixture["filename"]
    image = cv2.imread(os.path.join(data_dir, filename), cv2.IMREAD_COLOR)
    if image is None:
        raise RuntimeError(f"failed to read {filename}")
    resized = cv2.resize(image, (640, 640), interpolation=cv2.INTER_LINEAR)
    detector.setInputSize((640, 640))
    _, faces = detector.detect(resized)
    if faces is None or len(faces) == 0:
        raise RuntimeError(f"no face detected in {filename}")
    face = max(faces, key=lambda row: float(row[14]))
    h, w = image.shape[:2]
    face_for_chip = face.copy()
    face_for_chip[0] *= w / 640.0
    face_for_chip[2] *= w / 640.0
    face_for_chip[1] *= h / 640.0
    face_for_chip[3] *= h / 640.0
    for point_index in range(5):
        face_for_chip[4 + point_index * 2] *= w / 640.0
        face_for_chip[5 + point_index * 2] *= h / 640.0
    aligned = recognizer.alignCrop(image, face_for_chip)
    embedding = recognizer.feature(aligned).reshape(-1).astype(np.float32)
    norm = float(np.linalg.norm(embedding))
    if norm > 0:
        embedding = embedding / norm
    outputs.append({
        "filename": filename,
        "bbox": [float(face[0] / 640.0), float(face[1] / 640.0), float(face[2] / 640.0), float(face[3] / 640.0)],
        "score": float(face[14]),
        "embedding": embedding.astype(float).tolist(),
    })
print(json.dumps(outputs, sort_keys=True))
"#,
    )
}

fn run_insightface_scrfd_arcface_reference(
    fixtures: &[FaceFixture],
) -> TestResult<Vec<ReferenceFaceOutput>> {
    run_reference_python(
        fixtures,
        &[
            ("MIRROR_FACE_DETECTOR_MODEL", data_path("det_10g.onnx")),
            ("MIRROR_FACE_EMBEDDING_MODEL", data_path("w600k_r50.onnx")),
        ],
        r#"
import cv2, json, os, numpy as np
from types import SimpleNamespace
from insightface.model_zoo import get_model

data_dir = os.environ["MIRROR_FACE_DATA_DIR"]
fixtures = json.loads(os.environ["MIRROR_FACE_FIXTURES"])
detector = get_model(os.environ["MIRROR_FACE_DETECTOR_MODEL"], providers=["CPUExecutionProvider"])
detector.prepare(ctx_id=-1, input_size=(640, 640), det_thresh=0.3)
recognizer = get_model(os.environ["MIRROR_FACE_EMBEDDING_MODEL"], providers=["CPUExecutionProvider"])
recognizer.prepare(ctx_id=-1)
outputs = []
for fixture in fixtures:
    filename = fixture["filename"]
    image = cv2.imread(os.path.join(data_dir, filename), cv2.IMREAD_COLOR)
    if image is None:
        raise RuntimeError(f"failed to read {filename}")
    h, w = image.shape[:2]
    bboxes, kpss = detector.detect(image, max_num=0, metric="default")
    if bboxes is None or len(bboxes) == 0:
        raise RuntimeError(f"no face detected in {filename}")
    index = int(np.argmax(bboxes[:, 4]))
    bbox = bboxes[index]
    face = SimpleNamespace(bbox=bbox[:4], kps=kpss[index])
    embedding = recognizer.get(image, face).reshape(-1).astype(np.float32)
    norm = float(np.linalg.norm(embedding))
    if norm > 0:
        embedding = embedding / norm
    outputs.append({
        "filename": filename,
        "bbox": [float(bbox[0] / w), float(bbox[1] / h), float((bbox[2] - bbox[0]) / w), float((bbox[3] - bbox[1]) / h)],
        "score": float(bbox[4]),
        "embedding": embedding.astype(float).tolist(),
    })
print(json.dumps(outputs, sort_keys=True))
"#,
    )
}

fn run_reference_python(
    fixtures: &[FaceFixture],
    extra_env: &[(&str, PathBuf)],
    script: &str,
) -> TestResult<Vec<ReferenceFaceOutput>> {
    let fixture_json = serde_json::to_string(
        &fixtures
            .iter()
            .map(|fixture| json!({ "filename": fixture.filename }))
            .collect::<Vec<_>>(),
    )?;
    let mut command = Command::new(reference_python());
    command
        .arg("-c")
        .arg(script)
        .env("MIRROR_FACE_DATA_DIR", data_dir())
        .env("MIRROR_FACE_FIXTURES", fixture_json);
    for (key, path) in extra_env {
        command.env(key, path);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "reference python failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('['))
        .ok_or_else(|| format!("reference python produced no JSON\nstdout:\n{stdout}"))?;
    Ok(serde_json::from_str(json_line)?)
}

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("data")
}

fn reference_python() -> PathBuf {
    if let Some(path) = env::var_os("MIRROR_FACE_REFERENCE_PYTHON") {
        return PathBuf::from(path);
    }
    let workspace_python = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".venv-onnx-probe")
        .join("bin")
        .join("python");
    if workspace_python.exists() {
        return workspace_python;
    }
    PathBuf::from("python3")
}

fn assert_reference_outputs_match(
    name: &str,
    backend: &[ReferenceFaceOutput],
    reference: &[ReferenceFaceOutput],
    min_bbox_iou: f32,
    min_embedding_cosine: f32,
) -> TestResult {
    if backend.len() != reference.len() {
        return Err(format!(
            "{name}: output count mismatch backend={} reference={}",
            backend.len(),
            reference.len()
        )
        .into());
    }
    let reference_by_filename = reference
        .iter()
        .map(|output| (output.filename.as_str(), output))
        .collect::<HashMap<_, _>>();
    let mut summary = Vec::new();
    for backend_output in backend {
        let reference_output = reference_by_filename
            .get(backend_output.filename.as_str())
            .ok_or_else(|| format!("{name}: missing reference for {}", backend_output.filename))?;
        let iou = bbox_iou_xywh(backend_output.bbox, reference_output.bbox);
        let cosine = cosine(&backend_output.embedding, &reference_output.embedding);
        summary.push(format!(
            "{} bbox_iou={iou:.6} embedding_cosine={cosine:.6} backend_bbox={:?} reference_bbox={:?} backend_score={:.6} reference_score={:.6}",
            backend_output.filename,
            backend_output.bbox,
            reference_output.bbox,
            backend_output.score,
            reference_output.score
        ));
        if iou < min_bbox_iou || cosine < min_embedding_cosine {
            return Err(format!(
                "{name}: reference mismatch for {}: bbox_iou={iou:.6} embedding_cosine={cosine:.6}\n{}",
                backend_output.filename,
                summary.join("\n")
            )
            .into());
        }
    }
    Ok(())
}

fn bbox_iou_xywh(left: [f32; 4], right: [f32; 4]) -> f32 {
    let left_x2 = left[0] + left[2];
    let left_y2 = left[1] + left[3];
    let right_x2 = right[0] + right[2];
    let right_y2 = right[1] + right[3];
    let inter_left = left[0].max(right[0]);
    let inter_top = left[1].max(right[1]);
    let inter_right = left_x2.min(right_x2);
    let inter_bottom = left_y2.min(right_y2);
    let inter_width = (inter_right - inter_left).max(0.0);
    let inter_height = (inter_bottom - inter_top).max(0.0);
    let inter_area = inter_width * inter_height;
    let left_area = left[2].max(0.0) * left[3].max(0.0);
    let right_area = right[2].max(0.0) * right[3].max(0.0);
    inter_area / (left_area + right_area - inter_area).max(f32::EPSILON)
}

fn normalize_vector_l2(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in values {
            *value /= norm;
        }
    }
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL, local data/ YuNet/SFace fixtures, and a system ONNX Runtime library"]
async fn backend_requests_index_real_face_fixtures_into_people_albums() -> TestResult {
    backend_requests_index_real_face_fixtures_with_manifests(
        yunet_manifest(),
        sface_manifest(),
        "models/face_detection_yunet_2023mar.onnx",
        data_path("face_detection_yunet_2023mar.onnx"),
        "models/face_recognition_sface_2021dec.onnx",
        data_path("face_recognition_sface_2021dec.onnx"),
    )
    .await
}

async fn backend_requests_index_real_face_fixtures_with_manifests(
    detection_manifest: ModelPackManifest,
    embedding_manifest: ModelPackManifest,
    detection_pack_relative_path: &'static str,
    detection_source_path: PathBuf,
    embedding_pack_relative_path: &'static str,
    embedding_source_path: PathBuf,
) -> TestResult {
    require_system_onnxruntime()?;

    let pool = support::fresh_owner_pool().await?;
    let storage_root = TempDir::new()?;
    let storage = ObjectStorage::local(storage_root.path())?;
    let session = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("face-backend-pipeline-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let mut config = Config::from_env()?;
    config.face_recognition_enabled = true;
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config,
                db: Some(pool.clone()),
                setup: SetupState::Disabled,
                storage: Some(storage.clone()),
            }))
            .configure(http::configure),
    )
    .await;

    let install_detection = actix_test::TestRequest::post()
        .uri("/model-packs")
        .cookie(face_test_session_cookie(&session))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(&detection_manifest)
        .to_request();
    let install_detection_response = actix_test::call_service(&app, install_detection).await;
    assert_eq!(install_detection_response.status(), StatusCode::CREATED);
    let install_detection_body: Value =
        actix_test::read_body_json(install_detection_response).await;
    let detection_pack_id = Uuid::parse_str(
        install_detection_body["model_pack_id"]
            .as_str()
            .ok_or("detection model_pack_id missing")?,
    )?;
    copy_model_fixture(
        storage_root.path(),
        detection_pack_id,
        detection_pack_relative_path,
        detection_source_path,
    )?;
    let detection_self_test = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{detection_pack_id}/self-test"))
        .cookie(face_test_session_cookie(&session))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "passed": true, "error_message": null }))
        .to_request();
    let detection_self_test_response = actix_test::call_service(&app, detection_self_test).await;
    assert_eq!(detection_self_test_response.status(), StatusCode::OK);
    let activate_detection = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{detection_pack_id}/activate"))
        .cookie(face_test_session_cookie(&session))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    let activate_detection_response = actix_test::call_service(&app, activate_detection).await;
    assert_eq!(activate_detection_response.status(), StatusCode::OK);

    let install_embedding = actix_test::TestRequest::post()
        .uri("/model-packs")
        .cookie(face_test_session_cookie(&session))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(&embedding_manifest)
        .to_request();
    let install_embedding_response = actix_test::call_service(&app, install_embedding).await;
    assert_eq!(install_embedding_response.status(), StatusCode::CREATED);
    let install_embedding_body: Value =
        actix_test::read_body_json(install_embedding_response).await;
    let embedding_pack_id = Uuid::parse_str(
        install_embedding_body["model_pack_id"]
            .as_str()
            .ok_or("embedding model_pack_id missing")?,
    )?;
    copy_model_fixture(
        storage_root.path(),
        embedding_pack_id,
        embedding_pack_relative_path,
        embedding_source_path,
    )?;
    let embedding_self_test = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{embedding_pack_id}/self-test"))
        .cookie(face_test_session_cookie(&session))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "passed": true, "error_message": null }))
        .to_request();
    let embedding_self_test_response = actix_test::call_service(&app, embedding_self_test).await;
    assert_eq!(embedding_self_test_response.status(), StatusCode::OK);
    let activate_embedding = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{embedding_pack_id}/activate"))
        .cookie(face_test_session_cookie(&session))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    let activate_embedding_response = actix_test::call_service(&app, activate_embedding).await;
    assert_eq!(activate_embedding_response.status(), StatusCode::OK);

    let fixtures = discover_numbered_face_fixtures()?;
    let expected_counts = expected_fixture_counts(&fixtures);
    let mut asset_labels: HashMap<Uuid, String> = HashMap::new();
    for fixture in &fixtures {
        let bytes = fs::read(data_path(&fixture.filename))?;
        let create = actix_test::TestRequest::post()
            .uri("/uploads")
            .cookie(face_test_session_cookie(&session))
            .insert_header(("x-csrf-token", session.csrf_token.expose()))
            .set_json(json!({
                "original_filename": fixture.filename,
                "expected_size": bytes.len(),
                "expected_blake3": blake3::hash(&bytes).to_hex().to_string(),
                "media_type": fixture.media_type,
                "client_upload_key": null,
            }))
            .to_request();
        let create_response = actix_test::call_service(&app, create).await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let create_body: Value = actix_test::read_body_json(create_response).await;
        let upload_id = Uuid::parse_str(
            create_body["upload_id"]
                .as_str()
                .ok_or("upload_id missing")?,
        )?;

        let put = actix_test::TestRequest::put()
            .uri(&format!("/uploads/{upload_id}/parts/0"))
            .cookie(face_test_session_cookie(&session))
            .insert_header(("x-csrf-token", session.csrf_token.expose()))
            .set_payload(bytes)
            .to_request();
        let put_response = actix_test::call_service(&app, put).await;
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let complete = actix_test::TestRequest::post()
            .uri(&format!("/uploads/{upload_id}/complete"))
            .cookie(face_test_session_cookie(&session))
            .insert_header(("x-csrf-token", session.csrf_token.expose()))
            .to_request();
        let complete_response = actix_test::call_service(&app, complete).await;
        assert_eq!(complete_response.status(), StatusCode::OK);
        let complete_body: Value = actix_test::read_body_json(complete_response).await;
        let asset_id = Uuid::parse_str(
            complete_body["promoted"]["asset_id"]
                .as_str()
                .ok_or("promoted asset_id missing")?,
        )?;
        asset_labels.insert(asset_id, fixture.label.clone());
    }

    let runtime: SharedFaceRuntime = Arc::new(OnnxFaceRuntime::new(
        storage_root.path().to_path_buf(),
        MlDevicePreference::CpuOnly,
    ));
    let ml_runtime = MlRuntime::new(Arc::new(FakeImageTextEmbedder), std::num::NonZeroUsize::MIN);
    for _ in 0..fixtures.len() {
        let step = run_once(
            &pool,
            WorkerHandlers {
                storage: &storage,
                image_processor: &FakeImageProcessor,
                video_processor: &FakeVideoProcessor,
                ml_runtime: &ml_runtime,
                face_runtime: &runtime,
                job_kinds: &[JobKind::IndexFaces],
            },
            "real-face-worker",
            WorkerPolicy::production(),
        )
        .await?;
        assert_eq!(step, WorkerStep::Completed);
    }

    let people_request = actix_test::TestRequest::get()
        .uri("/people")
        .cookie(face_test_session_cookie(&session))
        .to_request();
    let people_response = actix_test::call_service(&app, people_request).await;
    assert_eq!(people_response.status(), StatusCode::OK);
    let people_body: Value = actix_test::read_body_json(people_response).await;
    let people = people_body
        .as_array()
        .ok_or("people response should be an array")?;

    let mut seen_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut album_summaries = Vec::new();
    for person in people {
        let person_id = person["person_id"]
            .as_str()
            .ok_or("person_id missing from people response")?;
        let faces_request = actix_test::TestRequest::get()
            .uri(&format!("/people/{person_id}/faces"))
            .cookie(face_test_session_cookie(&session))
            .to_request();
        let faces_response = actix_test::call_service(&app, faces_request).await;
        assert_eq!(faces_response.status(), StatusCode::OK);
        let faces_body: Value = actix_test::read_body_json(faces_response).await;
        let faces = faces_body
            .as_array()
            .ok_or("person faces response should be an array")?;
        let labels = faces
            .iter()
            .map(|face| {
                assert_eq!(face["chip_available"].as_bool(), Some(true));
                let raw_asset_id = face["asset_id"].as_str().ok_or("face asset_id missing")?;
                let asset_id = Uuid::parse_str(raw_asset_id)?;
                asset_labels
                    .get(&asset_id)
                    .cloned()
                    .ok_or_else(|| format!("unexpected asset id {asset_id}").into())
            })
            .collect::<TestResult<Vec<String>>>()?;
        let first_label = labels.first().ok_or("people album should not be empty")?;
        let expected_count = expected_counts
            .get(first_label)
            .ok_or_else(|| format!("unexpected people album label {first_label}"))?;
        assert_eq!(labels.len(), *expected_count);
        assert!(
            labels.iter().all(|label| label == first_label),
            "one people album should not mix fixture identities: {labels:?}",
        );
        album_summaries.push(format!("{first_label}:{}:{labels:?}", labels.len()));
        let first_face_id = faces[0]["face_id"]
            .as_str()
            .ok_or("face_id missing from people album")?;
        let chip_request = actix_test::TestRequest::get()
            .uri(&format!("/people/faces/{first_face_id}/chip"))
            .cookie(face_test_session_cookie(&session))
            .to_request();
        let chip_response = actix_test::call_service(&app, chip_request).await;
        assert_eq!(chip_response.status(), StatusCode::OK);
        assert_eq!(
            chip_response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("image/webp")
        );
        let chip_body = actix_test::read_body(chip_response).await;
        assert!(!chip_body.is_empty());
        *seen_counts.entry(first_label.clone()).or_default() += labels.len();
    }
    assert_eq!(people.len(), expected_counts.len(), "{album_summaries:?}");
    let mut face_counts = people
        .iter()
        .map(|person| person["face_count"].as_i64().unwrap_or_default())
        .collect::<Vec<_>>();
    face_counts.sort_unstable();
    let mut expected_face_counts = expected_counts
        .values()
        .map(|count| i64::try_from(*count))
        .collect::<Result<Vec<_>, _>>()?;
    expected_face_counts.sort_unstable();
    assert_eq!(face_counts, expected_face_counts, "{album_summaries:?}");
    assert_eq!(seen_counts, expected_counts);

    let unassigned_request = actix_test::TestRequest::get()
        .uri("/people/faces/unassigned")
        .cookie(face_test_session_cookie(&session))
        .to_request();
    let unassigned_response = actix_test::call_service(&app, unassigned_request).await;
    assert_eq!(unassigned_response.status(), StatusCode::OK);
    let unassigned_body: Value = actix_test::read_body_json(unassigned_response).await;
    assert_eq!(unassigned_body.as_array().map(Vec::len), Some(0));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL, local data/ InsightFace SCRFD/ArcFace fixtures, and a system ONNX Runtime library"]
async fn backend_requests_index_real_insightface_fixtures_into_people_albums() -> TestResult {
    backend_requests_index_real_face_fixtures_with_manifests(
        scrfd_manifest(),
        arcface_manifest(),
        "models/det_10g.onnx",
        data_path("det_10g.onnx"),
        "models/w600k_r50.onnx",
        data_path("w600k_r50.onnx"),
    )
    .await
}

fn require_system_onnxruntime() -> TestResult {
    let runtime_path = env::var_os("ORT_DYLIB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/lib/libonnxruntime.so"));
    if !runtime_path.is_absolute() {
        return Err(format!(
            "ORT_DYLIB_PATH must be an absolute system library path, got {}",
            runtime_path.display()
        )
        .into());
    }
    if runtime_path
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with(".venv"))
    {
        return Err(
            "face runtime test must use a system ONNX Runtime library, not a repo-local or Python wheel library"
                .into(),
        );
    }
    if !runtime_path.exists() {
        return Err(format!(
            "system ONNX Runtime library is missing at {}; install onnxruntime-cpu or set ORT_DYLIB_PATH",
            runtime_path.display()
        )
        .into());
    }
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let runtime_path = runtime_path.canonicalize()?;
    if runtime_path.starts_with(&workspace_root) {
        return Err(
            "face runtime test must use a system ONNX Runtime library, not a repo-local or Python wheel library"
                .into(),
        );
    }

    let ldd = Command::new("ldd").arg(&runtime_path).output()?;
    let stdout = String::from_utf8_lossy(&ldd.stdout);
    let stderr = String::from_utf8_lossy(&ldd.stderr);
    if !ldd.status.success() || stdout.contains("not found") || stderr.contains("not found") {
        return Err(format!(
            "system ONNX Runtime dependencies are incomplete for {}\nstdout:\n{}\nstderr:\n{}",
            runtime_path.display(),
            stdout,
            stderr
        )
        .into());
    }
    ort::init_from(&runtime_path)
        .map_err(|error| {
            format!(
                "failed to load system ONNX Runtime from {}: {error}",
                runtime_path.display()
            )
        })?
        .commit();
    Ok(())
}

fn copy_model_fixture(
    storage_root: &Path,
    model_pack_id: Uuid,
    relative_path: &str,
    source_path: PathBuf,
) -> TestResult {
    let target = storage_root
        .join("model-packs")
        .join(model_pack_id.to_string())
        .join(relative_path);
    fs::create_dir_all(target.parent().ok_or("model target has no parent")?)?;
    fs::copy(source_path, target)?;
    Ok(())
}

fn data_path(filename: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("data")
        .join(filename)
}

fn discover_numbered_face_fixtures() -> TestResult<Vec<FaceFixture>> {
    let data_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("data");
    let mut fixtures = Vec::new();
    for entry in fs::read_dir(&data_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else {
            continue;
        };
        let path = Path::new(filename);
        let Some(media_type) = fixture_media_type(path) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some((label, number)) = split_numbered_fixture_stem(stem) else {
            continue;
        };
        if !(1..=10_000).contains(&number) {
            continue;
        }
        fixtures.push((label.to_owned(), number, filename.to_owned(), media_type));
    }
    fixtures.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    let fixtures = fixtures
        .into_iter()
        .map(|(label, _, filename, media_type)| FaceFixture {
            filename,
            label,
            media_type,
        })
        .collect::<Vec<_>>();
    if fixtures.is_empty() {
        return Err(format!(
            "no numbered face fixtures found under {}; expected files like person1.jpg",
            data_root.display()
        )
        .into());
    }
    if expected_fixture_counts(&fixtures).len() < 2 {
        return Err("face fixture set should contain at least two people".into());
    }
    Ok(fixtures)
}

fn split_numbered_fixture_stem(stem: &str) -> Option<(&str, u32)> {
    let split_at = stem
        .trim_end_matches(|value: char| value.is_ascii_digit())
        .len();
    if split_at == 0 || split_at == stem.len() {
        return None;
    }
    let label = &stem[..split_at];
    let number = stem[split_at..].parse().ok()?;
    Some((label, number))
}

fn fixture_media_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        _ => None,
    }
}

fn expected_fixture_counts(fixtures: &[FaceFixture]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for fixture in fixtures {
        *counts.entry(fixture.label.clone()).or_default() += 1;
    }
    counts
}

fn face_test_session_cookie(session: &SessionCreateOutput) -> Cookie<'static> {
    Cookie::new("mirror_session", session.token.expose().to_owned())
}

fn yunet_manifest() -> ModelPackManifest {
    let model_path = "models/face_detection_yunet_2023mar.onnx".to_owned();
    ModelPackManifest {
        kind: "face_detection".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "opencv-yunet".to_owned(),
        model_revision: "2023mar".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 1,
        distance_metric: "cosine".to_owned(),
        onnx: dummy_onnx_config(),
        image_preprocess: ImagePreprocessConfig {
            width: 640,
            height: 640,
            color_order: "bgr".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.0, 0.0, 0.0],
            std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
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
            score_threshold: 0.5,
            min_face_size_ratio: 0.15,
            nms_threshold: 0.3,
            max_faces: 8,
        }),
        face_embedding: None,
        files: face_fixture_manifest_files(&model_path),
        self_tests: self_tests(),
    }
}

fn sface_manifest() -> ModelPackManifest {
    let model_path = "models/face_recognition_sface_2021dec.onnx".to_owned();
    ModelPackManifest {
        kind: "face_embedding".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "opencv-sface".to_owned(),
        model_revision: "2021dec".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 128,
        distance_metric: "cosine".to_owned(),
        onnx: dummy_onnx_config(),
        image_preprocess: ImagePreprocessConfig {
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.0, 0.0, 0.0],
            std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
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
            mean: [0.0, 0.0, 0.0],
            std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
            match_threshold: 0.363,
            l2_normalize_output: true,
        }),
        files: face_fixture_manifest_files(&model_path),
        self_tests: self_tests(),
    }
}

fn scrfd_manifest() -> ModelPackManifest {
    let model_path = "models/det_10g.onnx".to_owned();
    ModelPackManifest {
        kind: "face_detection".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "insightface-scrfd-10g".to_owned(),
        model_revision: "buffalo_l-v0.7".to_owned(),
        license: "model-license-required".to_owned(),
        embedding_dimension: 1,
        distance_metric: "cosine".to_owned(),
        onnx: dummy_onnx_config(),
        image_preprocess: ImagePreprocessConfig {
            width: 640,
            height: 640,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
        },
        face_detection: Some(FaceDetectionModelConfig {
            adapter: "scrfd".to_owned(),
            model_path: model_path.clone(),
            input_name: "input.1".to_owned(),
            boxes_output_name: "unused_boxes".to_owned(),
            scores_output_name: "unused_scores".to_owned(),
            landmarks_output_name: None,
            output_names: vec![
                "448".to_owned(),
                "471".to_owned(),
                "494".to_owned(),
                "451".to_owned(),
                "474".to_owned(),
                "497".to_owned(),
                "454".to_owned(),
                "477".to_owned(),
                "500".to_owned(),
            ],
            box_coordinate_space: "pixel".to_owned(),
            box_format: "xyxy".to_owned(),
            score_threshold: 0.3,
            min_face_size_ratio: 0.15,
            nms_threshold: 0.4,
            max_faces: 16,
        }),
        face_embedding: None,
        files: face_fixture_manifest_files(&model_path),
        self_tests: self_tests(),
    }
}

fn arcface_manifest() -> ModelPackManifest {
    let model_path = "models/w600k_r50.onnx".to_owned();
    ModelPackManifest {
        kind: "face_embedding".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "insightface-arcface-w600k-r50".to_owned(),
        model_revision: "buffalo_l-v0.7".to_owned(),
        license: "model-license-required".to_owned(),
        embedding_dimension: 512,
        distance_metric: "cosine".to_owned(),
        onnx: dummy_onnx_config(),
        image_preprocess: ImagePreprocessConfig {
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.5, 0.5, 0.5],
            std: [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0],
        },
        face_detection: None,
        face_embedding: Some(FaceEmbeddingModelConfig {
            adapter: "arcface".to_owned(),
            model_path: model_path.clone(),
            input_name: "input.1".to_owned(),
            output_name: "683".to_owned(),
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            alignment: "five_point".to_owned(),
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
            match_threshold: 0.55,
            l2_normalize_output: true,
        }),
        files: face_fixture_manifest_files(&model_path),
        self_tests: self_tests(),
    }
}

fn dummy_onnx_config() -> OnnxModelPackConfig {
    OnnxModelPackConfig {
        image_model_path: "unused/image.onnx".to_owned(),
        text_model_path: "unused/text.onnx".to_owned(),
        tokenizer_path: "unused/tokenizer.json".to_owned(),
        image_input_name: "image".to_owned(),
        image_output_name: "image_embedding".to_owned(),
        text_input_ids_name: "input_ids".to_owned(),
        text_attention_mask_name: "attention_mask".to_owned(),
        text_output_name: "text_embedding".to_owned(),
    }
}

fn face_fixture_file_manifest(path: &str) -> ModelPackFileManifest {
    ModelPackFileManifest {
        path: path.to_owned(),
        sha256: "0".repeat(64),
        size_bytes: 1,
    }
}

fn face_fixture_manifest_files(model_path: &str) -> Vec<ModelPackFileManifest> {
    vec![
        face_fixture_file_manifest(model_path),
        face_fixture_file_manifest("unused/image.onnx"),
        face_fixture_file_manifest("unused/text.onnx"),
        face_fixture_file_manifest("unused/tokenizer.json"),
    ]
}

fn self_tests() -> Vec<ModelPackSelfTestManifest> {
    vec![ModelPackSelfTestManifest {
        name: "local_fixture".to_owned(),
        input_path: "fixtures/input.jpg".to_owned(),
        expected_output_sha256: "0".repeat(64),
    }]
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    dot / left_norm.sqrt() / right_norm.sqrt()
}
