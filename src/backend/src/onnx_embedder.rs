//! ONNX image/text embedding runtime.
//!
//! This module owns model runtime mechanics only. Mirror side effects stay in
//! `ml`, and model-pack validation stays in `models`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use image::imageops::FilterType;
use ort::{session::Session, value::Tensor};
use tokenizers::Tokenizer;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::MlDevicePreference,
    media::{MediaToolError, decode_still_image, normalize_still_image_for_image_crate},
    ml::{EmbedImageRequest, EmbedTextRequest, ImageTextEmbedder, MlError},
    models::{ModelPackError, ModelPackKind, ModelPackManifest, ModelRuntime},
    storage::StorageKey,
};

/// Optional ONNX session execution overrides.
///
/// `None` leaves the corresponding ONNX Runtime default untouched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OnnxSessionOptions {
    /// Threads used to parallelize work within an operator.
    pub intra_threads: Option<usize>,
    /// Threads used to parallelize independent graph operators.
    pub inter_threads: Option<usize>,
    /// Whether independent graph operators may execute in parallel.
    pub parallel_execution: Option<bool>,
}

/// Production image/text embedder backed by ONNX Runtime.
pub struct OnnxImageTextEmbedder {
    storage_root: PathBuf,
    device: MlDevicePreference,
    session_options: OnnxSessionOptions,
    heif_convert_path: PathBuf,
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
        Self::with_session_options(storage_root, device, OnnxSessionOptions::default())
    }

    /// Creates a lazy ONNX runtime with explicit session execution overrides.
    #[must_use]
    pub fn with_session_options(
        storage_root: PathBuf,
        device: MlDevicePreference,
        session_options: OnnxSessionOptions,
    ) -> Self {
        Self::with_session_options_and_heif_converter(
            storage_root,
            device,
            session_options,
            "heif-convert",
        )
    }

    /// Creates a lazy ONNX runtime with an explicit HEIC/HEIF converter.
    #[must_use]
    pub fn with_heif_converter(
        storage_root: PathBuf,
        device: MlDevicePreference,
        heif_convert_path: impl Into<PathBuf>,
    ) -> Self {
        Self::with_session_options_and_heif_converter(
            storage_root,
            device,
            OnnxSessionOptions::default(),
            heif_convert_path,
        )
    }

    /// Creates a lazy runtime with explicit session and HEIF conversion settings.
    #[must_use]
    pub fn with_session_options_and_heif_converter(
        storage_root: PathBuf,
        device: MlDevicePreference,
        session_options: OnnxSessionOptions,
        heif_convert_path: impl Into<PathBuf>,
    ) -> Self {
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
            session_options,
            heif_convert_path: heif_convert_path.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn runtime_for(
        &self,
        model_pack_id: Uuid,
        manifest: &ModelPackManifest,
    ) -> Result<Arc<ModelPackRuntime>, MlError> {
        validate_semantic_onnx_manifest(manifest)?;

        {
            let cache = self.cache.lock().map_err(|_| MlError::RuntimeUnavailable)?;
            if let Some(runtime) = cache.get(&model_pack_id) {
                return Ok(Arc::clone(runtime));
            }
        }

        let runtime = Arc::new(ModelPackRuntime {
            image_session: Mutex::new(open_session_with_options(
                &model_pack_file_path(
                    &self.storage_root,
                    model_pack_id,
                    &manifest.onnx.image_model_path,
                )?,
                self.device,
                self.session_options,
            )?),
            text_session: Mutex::new(open_session_with_options(
                &model_pack_file_path(
                    &self.storage_root,
                    model_pack_id,
                    &manifest.onnx.text_model_path,
                )?,
                self.device,
                self.session_options,
            )?),
            tokenizer: Tokenizer::from_file(model_pack_file_path(
                &self.storage_root,
                model_pack_id,
                &manifest.onnx.tokenizer_path,
            )?)
            .map_err(|_| MlError::RuntimeUnavailable)?,
        });

        let mut cache = self.cache.lock().map_err(|_| MlError::RuntimeUnavailable)?;
        if let Some(existing) = cache.get(&model_pack_id) {
            return Ok(Arc::clone(existing));
        }
        cache.insert(model_pack_id, Arc::clone(&runtime));
        Ok(runtime)
    }
}

impl ImageTextEmbedder for OnnxImageTextEmbedder {
    fn embed_image(&self, request: EmbedImageRequest<'_>) -> Result<Vec<f32>, MlError> {
        let runtime = self.runtime_for(request.model_pack_id, request.manifest)?;
        let (shape, values) = preprocess_image_for_onnx(
            request.bytes,
            request.media_type,
            request.manifest,
            &self.heif_convert_path,
        )?;
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

pub(crate) fn open_session_with_options(
    path: &Path,
    device: MlDevicePreference,
    options: OnnxSessionOptions,
) -> Result<Session, MlError> {
    let mut builder = Session::builder().map_err(|_| MlError::RuntimeUnavailable)?;
    if let Some(intra_threads) = options.intra_threads {
        builder = builder
            .with_intra_threads(intra_threads)
            .map_err(|_| MlError::RuntimeUnavailable)?;
    }
    if let Some(inter_threads) = options.inter_threads {
        builder = builder
            .with_inter_threads(inter_threads)
            .map_err(|_| MlError::RuntimeUnavailable)?;
    }
    if let Some(parallel_execution) = options.parallel_execution {
        builder = builder
            .with_parallel_execution(parallel_execution)
            .map_err(|_| MlError::RuntimeUnavailable)?;
    }
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

pub(crate) fn extract_output(
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

pub(crate) fn model_pack_file_path(
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

pub(crate) fn preprocess_image_for_onnx(
    bytes: &[u8],
    media_type: &str,
    manifest: &ModelPackManifest,
    heif_convert_path: &Path,
) -> Result<(Vec<usize>, Vec<f32>), MlError> {
    let (bytes, media_type) =
        normalize_still_image_for_image_crate(bytes, media_type, heif_convert_path)
            .map_err(ml_image_error)?;
    let decoded = decode_still_image(&bytes, media_type).map_err(ml_image_error)?;
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
                    values[channel * width * height + y * width + x] = normalize_channel(
                        *raw,
                        manifest.image_preprocess.mean,
                        manifest.image_preprocess.std,
                        channel,
                    );
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
                    values[base + channel] = normalize_channel(
                        *raw,
                        manifest.image_preprocess.mean,
                        manifest.image_preprocess.std,
                        channel,
                    );
                }
            }
            Ok((vec![1, height, width, 3], values))
        }
        _ => Err(ModelPackError::InvalidManifest("image_preprocess.tensor_layout").into()),
    }
}

fn ml_image_error(error: MediaToolError) -> MlError {
    match error {
        MediaToolError::UnsupportedMediaType => MlError::UnsupportedMediaType,
        MediaToolError::Image(_) => MlError::InvalidImage,
        other => MlError::ImageConversion(other),
    }
}

pub(crate) fn ordered_channels(rgb: [u8; 3], bgr: bool) -> [u8; 3] {
    if bgr { [rgb[2], rgb[1], rgb[0]] } else { rgb }
}

pub(crate) fn normalize_channel(raw: u8, mean: [f32; 3], std: [f32; 3], channel: usize) -> f32 {
    let scaled = f32::from(raw) / 255.0;
    (scaled - mean[channel]) / std[channel]
}
