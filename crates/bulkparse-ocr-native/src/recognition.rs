//! Explicit PP-OCRv6 preprocessing. Model families do not share normalization
//! defaults; recognition and orientation retain their own adapter contracts.
use crate::{bad, Models};
use bulkparse::{Error, PdfOcrLine};
use image::RgbImage;
use oar_ocr::{
    core::{
        config::{onnx::OrtSessionConfig, ModelInferenceConfig},
        inference::OrtInfer,
        traits::{
            adapter::{AdapterBuilder, ModelAdapter},
            task::ImageTaskInput,
            OrtConfigurable,
        },
    },
    domain::adapters::{
        TextLineOrientationAdapter, TextLineOrientationAdapterBuilder, TextRecognitionAdapter,
        TextRecognitionAdapterBuilder,
    },
    models::detection::db::DBModel,
    oarocr::{EdgeProcessor, TextCroppingProcessor},
    processors::{
        types::ColorOrder, BoxType, DBPostProcess, DetResizeForTest, LimitType, NormalizeImage,
        ScoreMode, TensorLayout,
    },
};
use std::sync::Arc;

pub(crate) struct Engine {
    detector: DBModel,
    recognizer: TextRecognitionAdapter,
    orientation: Option<TextLineOrientationAdapter>,
}
impl Engine {
    pub fn load(models: &Models, dictionary: String, ort: OrtSessionConfig) -> Result<Self, Error> {
        let inference = OrtInfer::from_config(
            &ModelInferenceConfig {
                ort_session: Some(ort.clone()),
                ..Default::default()
            },
            &models.det.path,
            Some("x"),
        )
        .map_err(bad)?;
        let resizer = DetResizeForTest::new(
            None,
            None,
            None,
            Some(2560),
            Some(LimitType::Max),
            None,
            Some(4000),
        );
        let normalizer = detector_normalizer()?;
        let postprocessor = DBPostProcess::new(
            Some(0.3),
            Some(0.5),
            Some(1000),
            Some(1.6),
            Some(true),
            Some(ScoreMode::Fast),
            Some(BoxType::Quad),
        );
        let recognizer = TextRecognitionAdapterBuilder::new()
            .character_dict(dictionary.lines().map(str::to_owned).collect())
            .with_ort_config(ort.clone())
            .build(&models.rec.path)
            .map_err(bad)?;
        let orientation = models
            .cls
            .as_ref()
            .map(|model| {
                TextLineOrientationAdapterBuilder::new()
                    .with_ort_config(ort.clone())
                    .build(&model.path)
                    .map_err(bad)
            })
            .transpose()?;
        Ok(Self {
            detector: DBModel::new(inference, resizer, normalizer, postprocessor),
            recognizer,
            orientation,
        })
    }
    pub fn recognize(
        &self,
        image: &Arc<RgbImage>,
        check: &impl Fn() -> Result<(), Error>,
    ) -> Result<Vec<PdfOcrLine>, Error> {
        check()?;
        let detected = self
            .detector
            .forward(vec![image.as_ref().clone()], 0.3, 0.5, 1.6)
            .map_err(bad)?;
        check()?;
        let boxes = detected
            .boxes
            .into_iter()
            .next()
            .ok_or_else(|| bad("OCR returned no detection page"))?;
        let boxes = oar_ocr::processors::sort_quad_boxes(&boxes);
        let cropper = TextCroppingProcessor::new(true);
        let mut lines = Vec::new();
        for bbox in boxes {
            check()?;
            if bbox.points.len() != 4
                || bbox
                    .points
                    .iter()
                    .any(|p| !p.x.is_finite() || !p.y.is_finite())
            {
                return Err(bad("OCR returned invalid geometry"));
            }
            let crops = cropper
                .process((Arc::clone(image), vec![bbox.clone()]))
                .map_err(bad)?;
            let Some(mut crop) = crops.into_iter().next().flatten() else {
                return Err(bad("OCR text crop failed"));
            };
            if let Some(orientation) = &self.orientation {
                let result = orientation
                    .execute(
                        ImageTaskInput::from_arc_images(vec![Arc::clone(&crop)]),
                        None,
                    )
                    .map_err(bad)?;
                if result
                    .classifications
                    .first()
                    .and_then(|v| v.first())
                    .is_some_and(|v| v.class_id == 1)
                {
                    crop = Arc::new(image::imageops::rotate180(crop.as_ref()));
                }
            }
            check()?;
            if let Some(crop) = Arc::get_mut(&mut crop) {
                normalize_low_contrast(crop);
            }
            let recognized = self
                .recognizer
                .execute(ImageTaskInput::from_arc_images(vec![crop]), None)
                .map_err(bad)?;
            check()?;
            let (Some(text), Some(confidence)) = (
                recognized.texts.into_iter().next(),
                recognized.scores.into_iter().next(),
            ) else {
                return Err(bad("OCR returned incomplete recognition"));
            };
            if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                return Err(bad("OCR returned invalid confidence"));
            }
            if text.trim().is_empty() || confidence < 0.5 {
                continue;
            }
            let mut quad = [[0.; 2]; 4];
            for (out, p) in quad.iter_mut().zip(bbox.points) {
                *out = [
                    f64::from(p.x).clamp(0., image.width() as f64),
                    f64::from(p.y).clamp(0., image.height() as f64),
                ];
            }
            lines.push(PdfOcrLine {
                text,
                quad,
                confidence: confidence as f64,
            });
        }
        Ok(lines)
    }
}

// Lift only dim scans with compressed foreground/background levels. Percentiles
// ignore isolated noise; the range gate avoids amplifying nearly uniform patches
// or changing already high-contrast text. Work in the existing crop buffer.
fn normalize_low_contrast(image: &mut RgbImage) {
    let count = image.width() as usize * image.height() as usize;
    if count < 100 {
        return;
    }
    let mut histogram = [0usize; 256];
    let (mut dark, mut light) = (0, 0);
    for pixel in image.pixels() {
        let level =
            (u32::from(pixel[0]) * 299 + u32::from(pixel[1]) * 587 + u32::from(pixel[2]) * 114)
                / 1000;
        histogram[level as usize] += 1;
        dark += usize::from(level < 32);
        light += usize::from(level > 240);
        // Either extreme already outside the percentile gate proves this crop
        // cannot qualify; ordinary white-margin crops exit after about 1%.
        if dark > count / 100 || light > count / 100 {
            return;
        }
    }
    let percentile = |rank| {
        let mut total = 0;
        for (value, &frequency) in histogram.iter().enumerate() {
            total += frequency;
            if total > rank {
                return value;
            }
        }
        255
    };
    let low = percentile(count / 100);
    let high = percentile(count - 1 - count / 100);
    let span = high.saturating_sub(low);
    if low < 32 || high > 240 || !(64..=192).contains(&span) {
        return;
    }
    let mut lookup = [0u8; 256];
    for (value, out) in lookup.iter_mut().enumerate() {
        *out = ((value.saturating_sub(low) * 255 + span / 2) / span).min(255) as u8;
    }
    for value in image.as_mut() {
        *value = lookup[*value as usize];
    }
}

fn detector_normalizer() -> Result<NormalizeImage, Error> {
    NormalizeImage::with_color_order(
        Some(1. / 255.),
        Some(vec![0.5; 3]),
        Some(vec![0.5; 3]),
        Some(TensorLayout::CHW),
        Some(ColorOrder::BGR),
    )
    .map_err(bad)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contrast_preserves_normal_scans_and_does_not_amplify_flat_noise() {
        for (dark, light) in [(0, 255), (28, 255), (200, 220)] {
            let mut image = RgbImage::from_fn(100, 20, |x, _| {
                image::Rgb([if x < 20 { dark } else { light }; 3])
            });
            let before = image.clone();
            normalize_low_contrast(&mut image);
            assert_eq!(image, before);
        }
    }
    #[test]
    fn dim_scan_normalization_reuses_buffer_and_ignores_outliers() {
        let mut image = RgbImage::from_fn(100, 20, |x, _| {
            image::Rgb([if x < 20 { 70 } else { 225 }; 3])
        });
        image.put_pixel(0, 0, image::Rgb([0; 3]));
        image.put_pixel(99, 19, image::Rgb([255; 3]));
        let pointer = image.as_raw().as_ptr();
        normalize_low_contrast(&mut image);
        assert_eq!(image.as_raw().as_ptr(), pointer);
        assert_eq!(image.get_pixel(10, 10).0, [0; 3]);
        assert_eq!(image.get_pixel(80, 10).0, [255; 3]);
    }
    #[test]
    fn ppocr_v6_input_contract_is_bgr_minus_one_to_one() {
        let image = RgbImage::from_fn(3, 1, |x, _| {
            image::Rgb(match x {
                0 => [0, 0, 0],
                1 => [255, 255, 255],
                _ => [255, 0, 0],
            })
        });
        let tensor = detector_normalizer()
            .unwrap()
            .normalize_to(image::DynamicImage::ImageRgb8(image))
            .unwrap();
        for channel in 0..3 {
            assert_eq!(tensor[[0, channel, 0, 0]], -1.);
            assert_eq!(tensor[[0, channel, 0, 1]], 1.);
        }
        assert_eq!(tensor[[0, 0, 0, 2]], -1.);
        assert_eq!(tensor[[0, 2, 0, 2]], 1.);
    }
}
