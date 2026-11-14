//! Small isolated horizontal strokes often fall below text-detector thresholds.
//! Recover only pixel-supported dash marks inside otherwise empty table cells.
use image::RgbImage;

pub(crate) fn isolated_dash(
    image: &RgbImage,
    b: [f64; 4],
    origin: [u32; 2],
    text_height: f64,
) -> bool {
    let w = b[2] - b[0];
    let h = b[3] - b[1];
    if !(8. ..=256.).contains(&w)
        || !(8. ..=128.).contains(&h)
        || !(8. ..=128.).contains(&text_height)
    {
        return false;
    }
    let left = (b[0] + w * 0.15 + origin[0] as f64)
        .ceil()
        .clamp(0., image.width() as f64) as u32;
    let right = (b[2] - w * 0.15 + origin[0] as f64)
        .floor()
        .clamp(0., image.width() as f64) as u32;
    let top = (b[1] + h * 0.15 + origin[1] as f64)
        .ceil()
        .clamp(0., image.height() as f64) as u32;
    let bottom = (b[3] - h * 0.15 + origin[1] as f64)
        .floor()
        .clamp(0., image.height() as f64) as u32;
    let mut ink = [right, bottom, left, top];
    let mut count = 0;
    for y in top..bottom {
        for x in left..right {
            let p = image.get_pixel(x, y).0;
            if p.iter().all(|&c| c < 128) {
                ink[0] = ink[0].min(x);
                ink[1] = ink[1].min(y);
                ink[2] = ink[2].max(x);
                ink[3] = ink[3].max(y);
                count += 1;
            }
        }
    }
    if count == 0 || ink[0] <= left || ink[2] + 1 >= right || ink[1] <= top || ink[3] + 1 >= bottom
    {
        return false;
    }
    let iw = (ink[2] - ink[0] + 1) as f64;
    let ih = (ink[3] - ink[1] + 1) as f64;
    iw >= 3. * ih
        && iw >= text_height * 0.15
        && iw <= text_height * 0.9
        && ih <= text_height * 0.18
        && count as f64 >= iw * ih * 0.8
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_blank_cells_equals_signs_and_rules() {
        let mut image = RgbImage::from_pixel(100, 50, image::Rgb([255; 3]));
        let test = |im: &RgbImage| isolated_dash(im, [0., 0., 100., 50.], [0, 0], 24.);
        assert!(!test(&image));
        for y in 24..26 {
            for x in 45..55 {
                image.put_pixel(x, y, image::Rgb([0; 3]));
            }
        }
        assert!(test(&image));
        for y in 30..32 {
            for x in 45..55 {
                image.put_pixel(x, y, image::Rgb([0; 3]));
            }
        }
        assert!(!test(&image), "two strokes are an equals sign, not a dash");
        for x in 0..100 {
            image.put_pixel(x, 25, image::Rgb([0; 3]));
        }
        assert!(!test(&image), "a ruling line must never become content");
    }
}
