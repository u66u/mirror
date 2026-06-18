//! ONNX image/text embedding runtime.
//!
//! This module owns model runtime mechanics only. Mirror side effects stay in
//! `ml`, and model-pack validation stays in `models`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use image::{ImageFormat, imageops::FilterType};
use ort::{session::Session, value::Tensor};
use tokenizers::Tokenizer;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::MlDevicePreference,
    ml::{EmbedImageRequest, EmbedTextRequest, ImageTextEmbedder, MlError},
    models::{ModelPackError, ModelPackKind, ModelPackManifest, ModelRuntime},
    storage::StorageKey,
};

/// Production image/text embedder backed by ONNX Runtime.
pub struct OnnxImageTextEmbedder {
    storage_root: PathBuf,
    device: MlDevicePreference,
    cache: Mutex<HashMap<Uuid, Arc<ModelPackRuntime>>>,
}

struct ModelPackRuntime {
    image_session: Mutex<Session>,
    text_session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl OnnxImageTextEmbedder {
    /// Creates a lazy ONNX runtime. Sessions are opened on first use per model
    /// pack so worker startup does not require any installed packs.
    #[must_use]
    pub fn new(storage_root: PathBuf, device: MlDevicePreference) -> Self {
        match device {
            MlDevicePreference::GpuWithCpuFallback => {
                info!("ONNX runtime configured for GPU with explicit CPU fallback");
            }
            MlDevicePreference::CpuOnly => {
                info!("ONNX runtime configured for CPU only");
            }
            MlDevicePreference::GpuOnly => {
                warn!("ONNX runtime configured for GPU only; CPU fallback disabled");
            }
        }

        Self {
            storage_root,
            device,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn runtime_for(
        &self,
        model_pack_id: Uuid,
        manifest: &ModelPackManifest,
    ) -> Result<Arc<ModelPackRuntime>, MlError> {
        validate_semantic_onnx_manifest(manifest)?;

        let mut cache = self.cache.lock().map_err(|_| MlError::RuntimeUnavailable)?;
        if let Some(runtime) = cache.get(&model_pack_id) {
            return Ok(Arc::clone(runtime));
        }

        let runtime = Arc::new(ModelPackRuntime {
            image_session: Mutex::new(open_session(
                &model_pack_file_path(
                    &self.storage_root,
                    model_pack_id,
                    &manifest.onnx.image_model_path,
                )?,
                self.device,
            )?),
            text_session: Mutex::new(open_session(
                &model_pack_file_path(
                    &self.storage_root,
                    model_pack_id,
                    &manifest.onnx.text_model_path,
                )?,
                self.device,
            )?),
            tokenizer: Tokenizer::from_file(model_pack_file_path(
                &self.storage_root,
                model_pack_id,
                &manifest.onnx.tokenizer_path,
            )?)
            .map_err(|_| MlError::RuntimeUnavailable)?,
        });

        cache.insert(model_pack_id, Arc::clone(&runtime));
        Ok(runtime)
    }
}

impl ImageTextEmbedder for OnnxImageTextEmbedder {
    fn embed_image(&self, request: EmbedImageRequest<'_>) -> Result<Vec<f32>, MlError> {
        let runtime = self.runtime_for(request.model_pack_id, request.manifest)?;
        let (shape, values) =
            preprocess_image_for_onnx(request.bytes, request.media_type, request.manifest)?;
        let input = Tensor::from_array((shape, values)).map_err(|_| MlError::RuntimeUnavailable)?;
        let mut session = runtime
            .image_session
            .lock()
            .map_err(|_| MlError::RuntimeUnavailable)?;
        let outputs = session
            .run(ort::inputs! {
                request.manifest.onnx.image_input_name.as_str() => input
            })
            .map_err(|_| MlError::RuntimeUnavailable)?;
        extract_output(&outputs, &request.manifest.onnx.image_output_name)
    }

    fn embed_text(&self, request: EmbedTextRequest<'_>) -> Result<Vec<f32>, MlError> {
        let runtime = self.runtime_for(request.model_pack_id, request.manifest)?;
        let encoding = runtime
            .tokenizer
            .encode(request.text, true)
            .map_err(|_| MlError::RuntimeUnavailable)?;
        let ids = encoding
            .get_ids()
            .iter()
            .map(|value| i64::from(*value))
            .collect::<Vec<_>>();
        let attention_mask = encoding
            .get_attention_mask()
            .iter()
            .map(|value| i64::from(*value))
            .collect::<Vec<_>>();
        if ids.is_empty() || ids.len() != attention_mask.len() {
            return Err(MlError::InvalidTextQuery);
        }

        let shape = [1_usize, ids.len()];
        let input_ids =
            Tensor::from_array((shape, ids)).map_err(|_| MlError::RuntimeUnavailable)?;
        let attention_mask =
            Tensor::from_array((shape, attention_mask)).map_err(|_| MlError::RuntimeUnavailable)?;
        let mut session = runtime
            .text_session
            .lock()
            .map_err(|_| MlError::RuntimeUnavailable)?;
        let outputs = session
            .run(ort::inputs! {
                request.manifest.onnx.text_input_ids_name.as_str() => input_ids,
                request.manifest.onnx.text_attention_mask_name.as_str() => attention_mask
            })
            .map_err(|_| MlError::RuntimeUnavailable)?;
        extract_output(&outputs, &request.manifest.onnx.text_output_name)
    }
}

fn open_session(path: &Path, device: MlDevicePreference) -> Result<Session, MlError> {
    let mut builder = Session::builder().map_err(|_| MlError::RuntimeUnavailable)?;
    match device {
        MlDevicePreference::CpuOnly => {}
        MlDevicePreference::GpuWithCpuFallback => {
            builder = builder
                .with_execution_providers([ort::ep::CUDA::default().build()])
                .map_err(|_| MlError::RuntimeUnavailable)?;
        }
        MlDevicePreference::GpuOnly => {
            builder = builder
                .with_execution_providers([ort::ep::CUDA::default().build().error_on_failure()])
                .map_err(|_| MlError::RuntimeUnavailable)?;
        }
    }
    builder
        .commit_from_file(path)
        .map_err(|_| MlError::RuntimeUnavailable)
}

fn extract_output(
    outputs: &ort::session::SessionOutputs<'_>,
    output_name: &str,
) -> Result<Vec<f32>, MlError> {
    let output = outputs
        .get(output_name)
        .ok_or(ModelPackError::InvalidManifest("onnx.output_name"))?;
    let (_, values) = output
        .try_extract_tensor::<f32>()
        .map_err(|_| MlError::RuntimeUnavailable)?;
    Ok(values.to_vec())
}

fn model_pack_file_path(
    storage_root: &Path,
    model_pack_id: Uuid,
    relative_path: &str,
) -> Result<PathBuf, MlError> {
    let storage_key = StorageKey::model_pack_file(model_pack_id, relative_path)?;
    Ok(storage_root.join(storage_key.as_str()))
}

fn validate_semantic_onnx_manifest(manifest: &ModelPackManifest) -> Result<(), MlError> {
    let validated = crate::models::validate_model_pack_manifest(manifest)?;
    if validated.kind != ModelPackKind::SemanticImageText || validated.runtime != ModelRuntime::Onnx
    {
        return Err(ModelPackError::InvalidManifest("kind").into());
    }
    Ok(())
}

fn preprocess_image_for_onnx(
    bytes: &[u8],
    media_type: &str,
    manifest: &ModelPackManifest,
) -> Result<(Vec<usize>, Vec<f32>), MlError> {
    let image_format = match media_type {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/png" => ImageFormat::Png,
        _ => return Err(MlError::UnsupportedMediaType),
    };
    let decoded = image::load_from_memory_with_format(bytes, image_format)
        .map_err(|_| MlError::InvalidImage)?;
    let resized = decoded.resize_exact(
        manifest.image_preprocess.width,
        manifest.image_preprocess.height,
        FilterType::Triangle,
    );
    let rgb = resized.to_rgb8();
    let width = usize::try_from(rgb.width()).map_err(|_| MlError::RuntimeUnavailable)?;
    let height = usize::try_from(rgb.height()).map_err(|_| MlError::RuntimeUnavailable)?;
    let mut values = vec![0.0_f32; width * height * 3];
    let bgr = manifest.image_preprocess.color_order == "bgr";

    match manifest.image_preprocess.tensor_layout.as_str() {
        "nchw" => {
            for (x, y, pixel) in rgb.enumerate_pixels() {
                let x = usize::try_from(x).map_err(|_| MlError::RuntimeUnavailable)?;
                let y = usize::try_from(y).map_err(|_| MlError::RuntimeUnavailable)?;
                let channels = ordered_channels(pixel.0, bgr);
                for (channel, raw) in channels.iter().enumerate() {
                    values[channel * width * height + y * width + x] =
                        normalize_channel(*raw, channel, manifest);
                }
            }
            Ok((vec![1, 3, height, width], values))
        }
        "nhwc" => {
            for (x, y, pixel) in rgb.enumerate_pixels() {
                let x = usize::try_from(x).map_err(|_| MlError::RuntimeUnavailable)?;
                let y = usize::try_from(y).map_err(|_| MlError::RuntimeUnavailable)?;
                let base = (y * width + x) * 3;
                let channels = ordered_channels(pixel.0, bgr);
                for (channel, raw) in channels.iter().enumerate() {
                    values[base + channel] = normalize_channel(*raw, channel, manifest);
                }
            }
            Ok((vec![1, height, width, 3], values))
        }
        _ => Err(ModelPackError::InvalidManifest("image_preprocess.tensor_layout").into()),
    }
}

fn ordered_channels(rgb: [u8; 3], bgr: bool) -> [u8; 3] {
    if bgr { [rgb[2], rgb[1], rgb[0]] } else { rgb }
}

fn normalize_channel(raw: u8, channel: usize, manifest: &ModelPackManifest) -> f32 {
    let scaled = f32::from(raw) / 255.0;
    (scaled - manifest.image_preprocess.mean[channel]) / manifest.image_preprocess.std[channel]
}

#[cfg(test)]
mod tests {
    use image::{ImageBuffer, ImageFormat, Rgb};

    use super::*;
    use crate::models::{ModelPackFileManifest, ModelPackSelfTestManifest, OnnxModelPackConfig};

    #[test]
    fn preprocess_image_honors_nchw_rgb_normalization() -> Result<(), Box<dyn std::error::Error>> {
        let manifest = test_manifest("nchw", "rgb");
        let bytes = png_bytes([[255, 0, 0], [0, 128, 255]])?;

        let (shape, values) = preprocess_image_for_onnx(&bytes, "image/png", &manifest)?;

        assert_eq!(shape, vec![1, 3, 1, 2]);
        assert_eq!(values, vec![1.0, 0.0, 0.0, 128.0 / 255.0, 0.0, 1.0]);
        Ok(())
    }

    #[test]
    fn preprocess_image_honors_nhwc_bgr_normalization() -> Result<(), Box<dyn std::error::Error>> {
        let mut manifest = test_manifest("nhwc", "bgr");
        manifest.image_preprocess.mean = [0.5, 0.5, 0.5];
        manifest.image_preprocess.std = [0.5, 0.5, 0.5];
        let bytes = png_bytes([[255, 0, 0], [0, 128, 255]])?;

        let (shape, values) = preprocess_image_for_onnx(&bytes, "image/png", &manifest)?;

        assert_eq!(shape, vec![1, 1, 2, 3]);
        assert_eq!(
            values,
            vec![-1.0, -1.0, 1.0, 1.0, (128.0 / 255.0 - 0.5) / 0.5, -1.0]
        );
        Ok(())
    }

    fn png_bytes(pixels: [[u8; 3]; 2]) -> Result<Vec<u8>, image::ImageError> {
        let mut image = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(2, 1);
        image.put_pixel(0, 0, Rgb(pixels[0]));
        image.put_pixel(1, 0, Rgb(pixels[1]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        image.write_to(&mut cursor, ImageFormat::Png)?;
        Ok(cursor.into_inner())
    }

    fn test_manifest(layout: &str, color_order: &str) -> ModelPackManifest {
        ModelPackManifest {
            kind: "semantic_image_text".to_owned(),
            runtime: "onnx".to_owned(),
            model_key: "test".to_owned(),
            model_revision: "1".to_owned(),
            license: "test".to_owned(),
            embedding_dimension: 3,
            distance_metric: "cosine".to_owned(),
            onnx: OnnxModelPackConfig {
                image_model_path: "image.onnx".to_owned(),
                text_model_path: "text.onnx".to_owned(),
                tokenizer_path: "tokenizer.json".to_owned(),
                image_input_name: "image".to_owned(),
                image_output_name: "image_embedding".to_owned(),
                text_input_ids_name: "input_ids".to_owned(),
                text_attention_mask_name: "attention_mask".to_owned(),
                text_output_name: "text_embedding".to_owned(),
            },
            image_preprocess: crate::models::ImagePreprocessConfig {
                width: 2,
                height: 1,
                color_order: color_order.to_owned(),
                tensor_layout: layout.to_owned(),
                mean: [0.0, 0.0, 0.0],
                std: [1.0, 1.0, 1.0],
            },
            files: vec![
                ModelPackFileManifest {
                    path: "image.onnx".to_owned(),
                    sha256: "0".repeat(64),
                    size_bytes: 1,
                },
                ModelPackFileManifest {
                    path: "text.onnx".to_owned(),
                    sha256: "0".repeat(64),
                    size_bytes: 1,
                },
                ModelPackFileManifest {
                    path: "tokenizer.json".to_owned(),
                    sha256: "0".repeat(64),
                    size_bytes: 1,
                },
            ],
            self_tests: vec![ModelPackSelfTestManifest {
                name: "image".to_owned(),
                input_path: "image.png".to_owned(),
                expected_output_sha256: "0".repeat(64),
            }],
        }
    }
}
