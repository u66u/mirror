use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
};

use mirror_backend::{
    config::MlDevicePreference,
    ml::{EmbedImageRequest, EmbedTextRequest, ImageTextEmbedder},
    models::{ModelPackManifest, validate_embedding_output},
    onnx_embedder::OnnxImageTextEmbedder,
};
use uuid::Uuid;

#[test]
fn onnx_runtime_fixture_embeds_image_and_text_when_configured() -> Result<(), Box<dyn Error>> {
    let Some(fixture) = OnnxFixture::from_env()? else {
        return Ok(());
    };
    let manifest: ModelPackManifest = serde_json::from_slice(&std::fs::read(&fixture.manifest)?)?;
    let image_bytes = std::fs::read(&fixture.image)?;
    let embedder = OnnxImageTextEmbedder::new(fixture.storage_root, MlDevicePreference::CpuOnly);

    let image_values = embedder.embed_image(EmbedImageRequest {
        bytes: &image_bytes,
        media_type: media_type_for_path(&fixture.image)?,
        model_pack_id: fixture.model_pack_id,
        manifest: &manifest,
    })?;
    validate_embedding_output(&manifest, image_values)?;

    let text_values = embedder.embed_text(EmbedTextRequest {
        text: &fixture.text,
        model_pack_id: fixture.model_pack_id,
        manifest: &manifest,
    })?;
    validate_embedding_output(&manifest, text_values)?;

    Ok(())
}

struct OnnxFixture {
    storage_root: PathBuf,
    model_pack_id: Uuid,
    manifest: PathBuf,
    image: PathBuf,
    text: String,
}

impl OnnxFixture {
    fn from_env() -> Result<Option<Self>, Box<dyn Error>> {
        let names = [
            "MIRROR_ONNX_RUNTIME_FIXTURE_STORAGE_ROOT",
            "MIRROR_ONNX_RUNTIME_FIXTURE_MODEL_PACK_ID",
            "MIRROR_ONNX_RUNTIME_FIXTURE_MANIFEST",
            "MIRROR_ONNX_RUNTIME_FIXTURE_IMAGE",
        ];
        if names.iter().all(|name| env::var_os(name).is_none()) {
            return Ok(None);
        }

        let storage_root = required_path("MIRROR_ONNX_RUNTIME_FIXTURE_STORAGE_ROOT")?;
        let model_pack_id = env::var("MIRROR_ONNX_RUNTIME_FIXTURE_MODEL_PACK_ID")?.parse()?;
        let manifest = required_path("MIRROR_ONNX_RUNTIME_FIXTURE_MANIFEST")?;
        let image = required_path("MIRROR_ONNX_RUNTIME_FIXTURE_IMAGE")?;
        let text = env::var("MIRROR_ONNX_RUNTIME_FIXTURE_TEXT")
            .unwrap_or_else(|_| "a family photo".to_owned());

        Ok(Some(Self {
            storage_root,
            model_pack_id,
            manifest,
            image,
            text,
        }))
    }
}

fn required_path(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    Ok(PathBuf::from(env::var(name)?))
}

fn media_type_for_path(path: &Path) -> Result<&'static str, Box<dyn Error>> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        Some("png") => Ok("image/png"),
        _ => Err("fixture image must be .jpg, .jpeg, or .png".into()),
    }
}
