use super::*;
use image::{DynamicImage, ImageFormat, Rgb, Rgba, RgbaImage};
use std::io::Cursor;

fn provider() -> NativeOcr {
    NativeOcr {
        config: Config::default(),
        runtime: Mutex::new(None),
    }
}
fn encoded(image: DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, format).unwrap();
    output.into_inner()
}

#[test]
fn formats_composite_transparency_and_reject_oversized_images() {
    for format in [
        ImageFormat::Png,
        ImageFormat::Gif,
        ImageFormat::WebP,
        ImageFormat::Tiff,
    ] {
        let bytes = encoded(
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(30, 20, Rgba([0, 0, 0, 0]))),
            format,
        );
        let (image, count) = provider().image(&bytes, 1, &|| Ok(())).unwrap();
        assert_eq!(count, 1);
        assert_eq!(image.dimensions(), (30, 20));
        assert!(image.pixels().all(|p| p.0 == [255; 3]));
        let mut limited = provider();
        limited.config.max_pixels = 599;
        assert!(limited.image(&bytes, 1, &|| Ok(())).is_err());
    }
    let bytes = encoded(
        DynamicImage::ImageRgb8(RgbImage::from_pixel(30, 20, Rgb([255; 3]))),
        ImageFormat::Jpeg,
    );
    assert_eq!(
        provider()
            .image(&bytes, 1, &|| Ok(()))
            .unwrap()
            .0
            .dimensions(),
        (30, 20)
    );
}

#[test]
fn tiff_selects_frames_without_copying_or_decoding_other_pixels() {
    let mut output = Cursor::new(Vec::new());
    {
        let mut encoder = tiff::encoder::TiffEncoder::new(&mut output).unwrap();
        encoder
            .write_image::<tiff::encoder::colortype::RGB8>(30, 20, &vec![255; 30 * 20 * 3])
            .unwrap();
        encoder
            .write_image::<tiff::encoder::colortype::RGB8>(40, 10, &vec![0; 40 * 10 * 3])
            .unwrap();
    }
    let bytes = output.into_inner();
    let (image, count) = provider().image(&bytes, 2, &|| Ok(())).unwrap();
    assert_eq!(count, 2);
    assert_eq!(image.dimensions(), (40, 10));
    assert!(image.pixels().all(|p| p.0 == [0; 3]));
    assert!(provider().image(&bytes, 3, &|| Ok(())).is_err());
    let mut limited = provider();
    limited.config.max_frames = 1;
    assert!(limited.image(&bytes, 1, &|| Ok(())).is_err());
    assert!(matches!(
        provider().image(&bytes, 1, &|| Err(Error::Cancelled)),
        Err(Error::Cancelled)
    ));
}

#[test]
fn animated_gif_selects_every_frame() {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for color in [[255, 255, 255, 255], [0, 0, 0, 255]] {
            encoder
                .encode_frame(image::Frame::new(RgbaImage::from_pixel(
                    30,
                    20,
                    Rgba(color),
                )))
                .unwrap();
        }
    }
    let (image, count) = provider().image(&bytes, 2, &|| Ok(())).unwrap();
    assert_eq!(count, 2);
    assert!(image.pixels().all(|p| p.0 == [0; 3]));
}

fn table() -> (RgbImage, Vec<bulkparse::PdfOcrLine>) {
    let mut image = RgbImage::from_pixel(1000, 600, Rgb([255; 3]));
    for y in [100, 200, 300, 400] {
        for yy in y - 1..=y + 1 {
            for x in 99..=901 {
                image.put_pixel(x, yy, Rgb([0; 3]));
            }
        }
    }
    for (x, start) in [(100, 100), (900, 100), (500, 200)] {
        for xx in x - 1..=x + 1 {
            for y in start..=400 {
                image.put_pixel(xx, y, Rgb([0; 3]));
            }
        }
    }
    let lines = [
        ("Report", 130., 130.),
        ("Product", 130., 230.),
        ("Amount", 530., 230.),
        ("Apples", 130., 330.),
        ("125", 530., 330.),
    ]
    .into_iter()
    .map(|(text, x, y)| bulkparse::PdfOcrLine {
        text: text.into(),
        quad: [[x, y], [x + 100., y], [x + 100., y + 30.], [x, y + 30.]],
        confidence: 0.99,
    })
    .collect();
    (image, lines)
}

#[test]
fn merged_cells_preserve_every_value_once() {
    let (image, lines) = table();
    let regions = tables::regions(&image, &lines).unwrap();
    assert_eq!(regions.len(), 1);
    let bulkparse::PdfOcrRegionContent::Table { rows, .. } = &regions[0].content else {
        panic!("expected table")
    };
    assert_eq!(rows[0][0].col_span, 2);
    assert_eq!(
        rows.iter()
            .flatten()
            .map(|cell| cell.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Report", "Product", "Amount", "Apples", "125"]
    );
    PdfOcrOutput::Regions {
        image_size: [1000, 600],
        regions,
    }
    .validate(1024)
    .unwrap();
}

#[test]
fn crossing_text_box_and_side_prose_keep_line_output() {
    let (image, mut lines) = table();
    lines[1].quad[1][0] = 550.;
    lines[1].quad[2][0] = 550.;
    assert!(tables::regions(&image, &lines).is_none());
    let (image, mut lines) = table();
    lines.push(bulkparse::PdfOcrLine {
        text: "Outside".into(),
        quad: [[920., 200.], [990., 200.], [990., 230.], [920., 230.]],
        confidence: 0.99,
    });
    assert!(tables::regions(&image, &lines).is_none());
}

#[test]
fn boxed_caption_and_unruled_page_are_not_tables() {
    let (mut image, lines) = table();
    for y in 98..=402 {
        for x in 98..=902 {
            image.put_pixel(x, y, Rgb([255; 3]));
        }
    }
    assert!(tables::regions(&image, &lines).is_none());
    for y in [100, 400] {
        for x in 100..=900 {
            image.put_pixel(x, y, Rgb([0; 3]));
        }
    }
    for x in [100, 900] {
        for y in 100..=400 {
            image.put_pixel(x, y, Rgb([0; 3]));
        }
    }
    assert!(tables::regions(&image, &lines).is_none());
}

#[test]
fn legacy_python_config_is_rejected() {
    assert!(NativeOcr::from_json(br#"{"executable":"python3","models":"/missing"}"#).is_err());
}
