use crate::{bad, NativeOcr};
use bulkparse::Error;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};

impl NativeOcr {
    pub(crate) fn image(
        &self,
        bytes: &[u8],
        page: u32,
        check: &impl Fn() -> Result<(), Error>,
    ) -> Result<(RgbImage, u32), Error> {
        if page == 0 || page > self.config.max_frames {
            return Err(Error::Limit("ocr_image_frames"));
        }
        let mut reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(bad)?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(self.config.max_pixels * 8);
        reader.limits(limits.clone());
        match reader.format() {
            Some(ImageFormat::Tiff) => {
                // Walk directories without decompressing other pages, then redirect only the
                // TIFF header to the selected directory. The input bytes stay borrowed.
                let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes)).map_err(bad)?;
                let mut count = 0;
                let mut offset = None;
                loop {
                    check()?;
                    count += 1;
                    if count > self.config.max_frames {
                        return Err(Error::Limit("ocr_image_frames"));
                    }
                    if count == page {
                        offset = decoder.ifd_pointer().map(|v| v.0);
                    }
                    if !decoder.more_images() {
                        break;
                    }
                    decoder.next_image().map_err(bad)?;
                }
                let offset = offset.ok_or_else(|| bad("image frame outside input"))?;
                let input = TiffPage::new(bytes, offset)?;
                let mut decoder =
                    image::codecs::tiff::TiffDecoder::new(BufReader::new(input)).map_err(bad)?;
                decoder.set_limits(limits).map_err(bad)?;
                Ok((self.decode(decoder)?, count))
            }
            Some(ImageFormat::Gif) => {
                let mut decoder =
                    image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(bad)?;
                decoder.set_limits(limits).map_err(bad)?;
                self.check_dimensions(&decoder)?;
                self.animation(decoder.into_frames(), page, check)
            }
            Some(ImageFormat::Png) => {
                let mut decoder =
                    image::codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(bad)?;
                decoder.set_limits(limits).map_err(bad)?;
                self.check_dimensions(&decoder)?;
                if decoder.is_apng().map_err(bad)? {
                    self.animation(decoder.apng().map_err(bad)?.into_frames(), page, check)
                } else if page == 1 {
                    Ok((self.decode(decoder)?, 1))
                } else {
                    Err(bad("image frame outside input"))
                }
            }
            Some(ImageFormat::WebP) => {
                let mut decoder =
                    image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(bad)?;
                decoder.set_limits(limits).map_err(bad)?;
                self.check_dimensions(&decoder)?;
                if decoder.has_animation() {
                    self.animation(decoder.into_frames(), page, check)
                } else if page == 1 {
                    Ok((self.decode(decoder)?, 1))
                } else {
                    Err(bad("image frame outside input"))
                }
            }
            _ if page == 1 => Ok((self.decode(reader.into_decoder().map_err(bad)?)?, 1)),
            _ => Err(bad("image frame outside input")),
        }
    }

    fn check_dimensions(&self, decoder: &impl ImageDecoder) -> Result<(), Error> {
        let (width, height) = decoder.dimensions();
        self.check_pixels(width, height)
    }

    fn decode(&self, mut decoder: impl ImageDecoder) -> Result<RgbImage, Error> {
        self.check_dimensions(&decoder)?;
        let orientation = decoder.orientation().map_err(bad)?;
        let mut image = DynamicImage::from_decoder(decoder).map_err(bad)?;
        image.apply_orientation(orientation);
        Ok(white_background(image))
    }

    fn animation(
        &self,
        frames: image::Frames<'_>,
        page: u32,
        check: &impl Fn() -> Result<(), Error>,
    ) -> Result<(RgbImage, u32), Error> {
        let mut selected = None;
        let mut count = 0;
        for frame in frames {
            check()?;
            count += 1;
            if count > self.config.max_frames {
                return Err(Error::Limit("ocr_image_frames"));
            }
            let frame = frame.map_err(bad)?.into_buffer();
            self.check_pixels(frame.width(), frame.height())?;
            if count == page {
                selected = Some(white_background(DynamicImage::ImageRgba8(frame)));
            }
        }
        Ok((
            selected.ok_or_else(|| bad("image frame outside input"))?,
            count,
        ))
    }
}

fn white_background(image: DynamicImage) -> RgbImage {
    if let DynamicImage::ImageRgb8(rgb) = image {
        return rgb;
    }
    let rgba = image.into_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let pixel = rgba.get_pixel(x, y);
        let alpha = u16::from(pixel[3]);
        image::Rgb(std::array::from_fn(|i| {
            ((u16::from(pixel[i]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
        }))
    })
}

struct TiffPage<'a> {
    input: Cursor<&'a [u8]>,
    header: Vec<u8>,
}
impl<'a> TiffPage<'a> {
    fn new(bytes: &'a [u8], offset: u64) -> Result<Self, Error> {
        let little = bytes.starts_with(b"II");
        let big_tiff = bytes.get(2..4) == Some(if little { &[43, 0] } else { &[0, 43] });
        let mut header = bytes
            .get(..if big_tiff { 16 } else { 8 })
            .ok_or_else(|| bad("truncated TIFF header"))?
            .to_vec();
        if big_tiff {
            header[8..16].copy_from_slice(&if little {
                offset.to_le_bytes()
            } else {
                offset.to_be_bytes()
            });
        } else {
            let offset = u32::try_from(offset).map_err(bad)?;
            header[4..8].copy_from_slice(&if little {
                offset.to_le_bytes()
            } else {
                offset.to_be_bytes()
            });
        }
        Ok(Self {
            input: Cursor::new(bytes),
            header,
        })
    }
}
impl Read for TiffPage<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let start = self.input.position();
        let count = self.input.read(buffer)?;
        if start < self.header.len() as u64 {
            let start = start as usize;
            let patched = count.min(self.header.len() - start);
            buffer[..patched].copy_from_slice(&self.header[start..start + patched]);
        }
        Ok(count)
    }
}
impl Seek for TiffPage<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.input.seek(position)
    }
}
