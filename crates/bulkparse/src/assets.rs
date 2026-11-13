//! Bounded original-image retention. Assets are an inventory, not reading order.
//! Never execute, rasterize, fetch, or OCR an image while collecting it.
use crate::{Context, Error, Format, ImageAsset, Source};
use sha2::{Digest, Sha256};

pub(crate) mod base64_bytes {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;
    pub fn serialize<S: Serializer>(data: &Arc<[u8]>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&base64::display::Base64Display::new(data, &STANDARD))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<[u8]>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD
            .decode(text)
            .map(Arc::from)
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) fn image_type(ext: &str) -> Option<(&'static str, &'static str)> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => ("image/png", "png"),
        "jpg" | "jpeg" => ("image/jpeg", "jpg"),
        "gif" => ("image/gif", "gif"),
        "webp" => ("image/webp", "webp"),
        "bmp" => ("image/bmp", "bmp"),
        "tif" | "tiff" => ("image/tiff", "tif"),
        "svg" => ("image/svg+xml", "svg"),
        "emf" => ("image/emf", "emf"),
        "wmf" => ("image/wmf", "wmf"),
        "jp2" => ("image/jp2", "jp2"),
        _ => return None,
    })
}
impl Context<'_> {
    pub(crate) fn image_inline(&mut self, part: &str, alt: &str) -> Option<crate::Inline> {
        let asset = self
            .assets
            .get(*self.image_index.get(part.trim_start_matches("./"))?)?;
        let url = format!("asset:{}", asset.id);
        Some(crate::Inline::Image {
            url,
            alt: alt.into(),
        })
    }
    pub(crate) fn image_remaining(&self) -> usize {
        self.options
            .limits
            .max_image_bytes
            .saturating_sub(self.image_bytes)
    }
    pub(crate) fn image_gap(&mut self, message: &str, source: Source) {
        self.diagnostic("image_asset_unavailable", message, source, true);
    }
    pub(crate) fn retain_image(
        &mut self,
        data: &[u8],
        ext: &str,
        source: Source,
    ) -> Result<(), Error> {
        self.check()?;
        if !self.options.extract_images || data.is_empty() {
            return Ok(());
        }
        let Some((mime, ext)) = image_type(ext) else {
            return Ok(());
        };
        // Hash before charging so repeated references share one allocation.
        let mut id = String::with_capacity(65 + ext.len());
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in Sha256::digest(data) {
            id.push(HEX[(byte >> 4) as usize] as char);
            id.push(HEX[(byte & 15) as usize] as char);
        }
        id.push('.');
        id.push_str(ext);
        if let Some(index) = self.assets.iter().position(|a| a.id == id) {
            if let Some(part) = &source.part {
                self.image_index.insert(part.to_string(), index);
            }
            return Ok(());
        }
        if data.len() > self.image_remaining()
            || self.assets.len() >= self.options.limits.max_images
        {
            self.image_gap(
                "Image retention limit reached; this image's bytes were omitted.",
                source,
            );
            return Ok(());
        }
        self.image_bytes += data.len();
        if let Some(part) = &source.part {
            self.image_index.insert(part.to_string(), self.assets.len());
        }
        self.assets.push(ImageAsset {
            id,
            media_type: mime.into(),
            source,
            data: data.into(),
        });
        Ok(())
    }
}

pub(crate) fn prepare(format: Format, bytes: &[u8], ctx: &mut Context<'_>) -> Result<(), Error> {
    if ctx.images_prepared || !ctx.options.extract_images {
        return Ok(());
    }
    #[cfg(any(feature = "office", feature = "docx"))]
    if matches!(
        format,
        Format::Docx
            | Format::Xlsx
            | Format::Xlsb
            | Format::Pptx
            | Format::Odt
            | Format::Ods
            | Format::Odp
            | Format::Epub
    ) {
        package(bytes, ctx)?;
    }
    #[cfg(feature = "office")]
    if format == Format::Rtf {
        rtf(bytes, ctx)?;
    }
    let _ = (format, bytes);
    ctx.images_prepared = true;
    Ok(())
}
pub(crate) fn collect(format: Format, bytes: &[u8], ctx: &mut Context<'_>) -> Result<(), Error> {
    prepare(format, bytes, ctx)?;
    if !ctx.assets.is_empty() {
        ctx.diagnostic("image_assets_retained", "Original image files retained. Known references preserve reading-flow placement; unplaced assets may include unused resources or alternate previews. Images have not been interpreted or OCRed. Text-only Markdown does not contain their bytes.", Source::default(), false);
    }
    Ok(())
}

#[cfg(any(feature = "office", feature = "docx"))]
fn package(bytes: &[u8], ctx: &mut Context<'_>) -> Result<(), Error> {
    use std::io::{Cursor, Read};
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::Malformed(e.to_string()))?;
    if zip.len() > ctx.options.limits.max_archive_entries {
        return Err(Error::Limit("max_archive_entries"));
    }
    // Inventory package image files without reading large worksheet/chapter XML again.
    // Origin names are metadata only; exports always use generated safe filenames.
    let mut names: Vec<_> = zip
        .file_names()
        .filter(|n| image_type(n.rsplit('.').next().unwrap_or("")).is_some())
        .map(str::to_owned)
        .collect();
    names.sort();
    let mut expanded = 0usize;
    for name in names {
        ctx.check()?;
        let source = Source {
            part: Some(name.clone().into()),
            ..Default::default()
        };
        let cap = ctx.options.limits.max_image_bytes.min(
            ctx.options
                .limits
                .max_expanded_bytes
                .saturating_sub(expanded),
        );
        let file = zip
            .by_name(&name)
            .map_err(|e| Error::Malformed(e.to_string()))?;
        if file.size() > cap as u64 {
            ctx.image_gap(
                "Package image exceeds the remaining image retention budget.",
                source,
            );
            continue;
        }
        let mut data = Vec::with_capacity(file.size() as usize);
        match file
            .take((cap as u64).saturating_add(1))
            .read_to_end(&mut data)
        {
            Ok(_) if data.len() <= cap => {
                expanded += data.len();
                ctx.retain_image(&data, name.rsplit('.').next().unwrap(), source)?;
            }
            _ => ctx.image_gap(
                "Package image could not be read within its declared bounds.",
                source,
            ),
        }
    }
    Ok(())
}

/// OfficeArt records, including nested containers and FBSE entries. No byte-pattern carving.
#[cfg(feature = "office")]
pub(crate) fn office_art(data: &[u8], origin: &str, ctx: &mut Context<'_>) -> Result<(), Error> {
    use std::io::Read;
    if !ctx.options.extract_images {
        return Ok(());
    }
    let mut ranges = vec![(0usize, data.len(), 0usize)];
    let mut visited = 0usize;
    while let Some((mut pos, end, depth)) = ranges.pop() {
        ctx.depth(depth)?;
        while pos < end {
            ctx.check()?;
            visited += 1;
            if visited > ctx.options.limits.max_nodes {
                return Err(Error::Limit("max_nodes"));
            }
            let source = Source {
                part: Some(if origin == "Pictures" {
                    format!("Pictures/{pos}").into()
                } else {
                    origin.into()
                }),
                byte_range: Some([pos, end].into()),
                ..Default::default()
            };
            let Some(h) = data.get(pos..end).and_then(|r| r.get(..8)) else {
                break;
            };
            let vi = u16::from_le_bytes([h[0], h[1]]);
            let kind = u16::from_le_bytes([h[2], h[3]]);
            let len = u32::from_le_bytes(h[4..8].try_into().unwrap()) as usize;
            let start = pos + 8;
            let Some(next) = start.checked_add(len).filter(|n| *n <= end) else {
                ctx.image_gap("Truncated OfficeArt image record.", source);
                break;
            };
            let body = &data[start..next];
            pos = next;
            if kind == 0xf007 {
                if let Some(n) = body.get(33) {
                    let offset = 36 + usize::from(*n);
                    if offset < body.len() {
                        ranges.push((start + offset, next, depth + 1));
                    }
                }
            } else if vi & 15 == 15 {
                ranges.push((start, next, depth + 1));
            } else if matches!(kind, 0xf01d | 0xf01e) {
                let instance = vi >> 4;
                let offset = if matches!(instance, 0x46b | 0x6e1 | 0x6e3) {
                    33
                } else {
                    17
                };
                if let Some(payload) = body.get(offset..) {
                    ctx.retain_image(payload, if kind == 0xf01d { "jpg" } else { "png" }, source)?;
                }
            } else if kind == 0xf01f {
                let offset = if vi >> 4 == 0x7a9 { 33 } else { 17 };
                if let Some(dib) = body.get(offset..) {
                    match bitmap_file(dib, ctx.image_remaining()) {
                        Ok(bytes) => ctx.retain_image(&bytes, "bmp", source)?,
                        Err(error) => ctx.image_gap(&error.to_string(), source),
                    }
                }
            } else if matches!(kind, 0xf01a | 0xf01b) {
                let offset = if matches!(vi >> 4, 0x3d5 | 0x217) {
                    32
                } else {
                    16
                };
                if let Some(b) = body.get(offset..).filter(|b| b.len() >= 34) {
                    let size = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
                    let stored = u32::from_le_bytes(b[28..32].try_into().unwrap()) as usize;
                    let ext = if kind == 0xf01a { "emf" } else { "wmf" };
                    if size > ctx.image_remaining() || stored != b.len() - 34 {
                        ctx.image_gap(
                            "Metafile exceeds its budget or has invalid size metadata.",
                            source,
                        );
                        continue;
                    }
                    match b[32] {
                        0xfe if size == stored => ctx.retain_image(&b[34..], ext, source)?,
                        0 => {
                            let mut decoded = Vec::new();
                            let result = flate2::read::ZlibDecoder::new(&b[34..])
                                .take(size as u64 + 1)
                                .read_to_end(&mut decoded);
                            if result.is_ok() && decoded.len() == size {
                                ctx.retain_image(&decoded, ext, source)?;
                            } else {
                                ctx.image_gap("Metafile decompression failed or disagreed with its declared size.", source);
                            }
                        }
                        _ => ctx.image_gap("Unsupported metafile compression.", source),
                    }
                }
            } else if (0xf018..=0xf029).contains(&kind) {
                ctx.image_gap("Unsupported OfficeArt image encoding.", source);
            }
        }
    }
    Ok(())
}

// Wrap a DIB in a BMP file header without decoding pixels or inventing a palette.
#[cfg(feature = "office")]
fn bitmap_file(dib: &[u8], cap: usize) -> Result<Vec<u8>, Error> {
    let bad = || Error::Malformed("Invalid or unsupported DIB header".into());
    let u16at = |p: usize| {
        dib.get(p..p + 2)
            .and_then(|b| b.try_into().ok())
            .map(u16::from_le_bytes)
            .ok_or_else(bad)
    };
    let u32at = |p: usize| {
        dib.get(p..p + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(bad)
    };
    let size = dib
        .len()
        .checked_add(14)
        .filter(|n| *n <= cap && *n <= u32::MAX as usize)
        .ok_or(Error::Limit("max_image_bytes"))?;
    let header = u32at(0)? as usize;
    let (bits, entries, entry_size, masks) = if header == 12 {
        let bits = u16at(10)?;
        (
            bits,
            if bits <= 8 { 1usize << bits } else { 0 },
            3usize,
            0usize,
        )
    } else if matches!(header, 40 | 52 | 56 | 108 | 124) {
        let bits = u16at(14)?;
        let colors = u32at(32)? as usize;
        let compression = u32at(16)?;
        let entries = if colors != 0 {
            colors
        } else if bits <= 8 {
            1usize << bits
        } else {
            0
        };
        (
            bits,
            entries,
            4,
            if header == 40 {
                match compression {
                    3 => 12,
                    6 => 16,
                    _ => 0,
                }
            } else {
                0
            },
        )
    } else {
        return Err(bad());
    };
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) || entries > 256 {
        return Err(bad());
    }
    let offset = header
        .checked_add(entries * entry_size)
        .and_then(|n| n.checked_add(masks))
        .filter(|n| *n <= dib.len())
        .ok_or_else(bad)?;
    let mut file = Vec::with_capacity(size);
    file.extend(b"BM");
    file.extend((size as u32).to_le_bytes());
    file.extend([0; 4]);
    file.extend(((offset + 14) as u32).to_le_bytes());
    file.extend(dib);
    Ok(file)
}

/// RTF pict destinations. Binary lengths take precedence over braces/control syntax.
#[cfg(feature = "office")]
fn rtf(b: &[u8], ctx: &mut Context<'_>) -> Result<(), Error> {
    #[derive(Default)]
    struct Picture {
        depth: usize,
        start: usize,
        ext: &'static str,
        bytes: Vec<u8>,
        nibble: Option<u8>,
        invalid: bool,
    }
    let mut picture: Option<Picture> = None;
    let mut depth = 0usize;
    let mut pos = 0usize;
    while pos < b.len() {
        ctx.check()?;
        match b[pos] {
            b'{' => {
                depth += 1;
                ctx.depth(depth)?;
                pos += 1;
            }
            b'}' => {
                if picture.as_ref().is_some_and(|p| p.depth == depth) {
                    let p = picture.take().unwrap();
                    let source = Source {
                        part: Some(format!("RTF/pict/{}", p.start).into()),
                        byte_range: Some([p.start, pos + 1].into()),
                        ..Default::default()
                    };
                    if p.invalid || p.nibble.is_some() || p.ext.is_empty() {
                        ctx.image_gap(
                            "RTF image encoding is unsupported, malformed, or exceeds its budget.",
                            source,
                        );
                    } else if p.ext == "dib" {
                        match bitmap_file(&p.bytes, ctx.image_remaining()) {
                            Ok(bytes) => ctx.retain_image(&bytes, "bmp", source)?,
                            Err(error) => ctx.image_gap(&error.to_string(), source),
                        }
                    } else {
                        ctx.retain_image(&p.bytes, p.ext, source)?;
                    }
                }
                depth = depth.saturating_sub(1);
                pos += 1;
            }
            b'\\' => {
                pos += 1;
                let begin = pos;
                while b.get(pos).is_some_and(u8::is_ascii_alphabetic) {
                    pos += 1;
                }
                if begin == pos {
                    if b.get(pos) == Some(&b'\'') {
                        pos = (pos + 3).min(b.len());
                    } else {
                        pos = (pos + 1).min(b.len());
                    }
                    continue;
                }
                let word = &b[begin..pos];
                let start = pos;
                if b.get(pos) == Some(&b'-') {
                    pos += 1;
                }
                while b.get(pos).is_some_and(u8::is_ascii_digit) {
                    pos += 1;
                }
                let number = std::str::from_utf8(&b[start..pos])
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok());
                if b.get(pos) == Some(&b' ') {
                    pos += 1;
                }
                if word == b"pict" && picture.is_none() {
                    picture = Some(Picture {
                        depth,
                        start: begin - 1,
                        ..Default::default()
                    });
                }
                if word == b"bin" {
                    let n =
                        number.ok_or_else(|| Error::Malformed("Invalid RTF binary size".into()))?;
                    let end = pos
                        .checked_add(n)
                        .filter(|p| *p <= b.len())
                        .ok_or_else(|| Error::Malformed("Truncated RTF binary image".into()))?;
                    if let Some(p) = picture.as_mut().filter(|p| p.depth == depth) {
                        if p.bytes.len().saturating_add(n) <= ctx.image_remaining() {
                            p.bytes.extend_from_slice(&b[pos..end]);
                        } else {
                            p.invalid = true;
                        }
                    }
                    pos = end;
                } else if let Some(p) = picture.as_mut().filter(|p| p.depth == depth) {
                    match word {
                        b"pngblip" => p.ext = "png",
                        b"jpegblip" => p.ext = "jpg",
                        b"emfblip" => p.ext = "emf",
                        b"wmetafile" => p.ext = "wmf",
                        b"dibitmap" => p.ext = "dib",
                        _ => {}
                    }
                }
            }
            c => {
                if let Some(p) = picture.as_mut().filter(|p| p.depth == depth && !p.invalid) {
                    if !c.is_ascii_whitespace() {
                        if let Some(n) = (c as char).to_digit(16) {
                            if let Some(high) = p.nibble.take() {
                                if p.bytes.len() < ctx.image_remaining() {
                                    p.bytes.push(high * 16 + n as u8);
                                } else {
                                    p.invalid = true;
                                }
                            } else {
                                p.nibble = Some(n as u8);
                            }
                        } else {
                            p.invalid = true;
                        }
                    }
                }
                pos += 1;
            }
        }
    }
    Ok(())
}

#[cfg(feature = "office")]
pub(crate) fn xls_images(bytes: &[u8], ctx: &mut Context<'_>) -> Result<(), Error> {
    if !ctx.options.extract_images {
        return Ok(());
    }
    let mut pos = 0usize;
    let mut art = Vec::new();
    let mut continuing = false;
    while pos + 4 <= bytes.len() {
        ctx.check()?;
        let kind = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap());
        let size = u16::from_le_bytes(bytes[pos + 2..pos + 4].try_into().unwrap()) as usize;
        let Some(data) = bytes.get(pos + 4..).and_then(|b| b.get(..size)) else {
            break;
        };
        if kind == 0x00eb || (kind == 0x003c && continuing) {
            if art.len().saturating_add(size) > ctx.options.limits.max_expanded_bytes {
                ctx.image_gap(
                    "Workbook image records exceed the expansion budget.",
                    Source::default(),
                );
                return Ok(());
            }
            art.extend_from_slice(data);
            continuing = true;
        } else {
            continuing = false;
        }
        pos += 4 + size;
    }
    office_art(&art, "Workbook/MSODRAWINGGROUP", ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "office")]
    fn record(vi: u16, kind: u16, data: &[u8]) -> Vec<u8> {
        [
            vi.to_le_bytes().as_slice(),
            kind.to_le_bytes().as_slice(),
            (data.len() as u32).to_le_bytes().as_slice(),
            data,
        ]
        .concat()
    }
    #[test]
    #[cfg(feature = "office")]
    fn dib_files_get_palette_aware_offsets_without_decoding_pixels() {
        let mut dib = vec![0; 44];
        dib[..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1u32.to_le_bytes());
        dib[8..12].copy_from_slice(&1u32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        let bmp = bitmap_file(&dib, 100).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
        assert_eq!(&bmp[14..], &dib);
        assert!(bitmap_file(&dib, 50).is_err());
        dib[14..16].copy_from_slice(&1u16.to_le_bytes());
        assert!(bitmap_file(&dib, 100).is_err()); // Missing the two-entry palette.
        dib.extend([0; 8]);
        let bmp = bitmap_file(&dib, 100).unwrap();
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 62);
    }
    #[test]
    #[cfg(feature = "office")]
    fn nested_office_art_and_split_biff_records_preserve_payloads() {
        let png = [vec![0; 17], b"PNG original".to_vec()].concat();
        let art = record(15, 0xf000, &record(0x6e00, 0xf01e, &png));
        let mut biff = Vec::new();
        for (kind, bytes) in [(0x00ebu16, &art[..11]), (0x003c, &art[11..])] {
            biff.extend(kind.to_le_bytes());
            biff.extend((bytes.len() as u16).to_le_bytes());
            biff.extend(bytes);
        }
        let options = crate::Options::default();
        let token = crate::CancellationToken::default();
        let mut ctx = Context::new(&options, &token);
        xls_images(&biff, &mut ctx).unwrap();
        assert_eq!(ctx.assets.len(), 1);
        assert_eq!(&*ctx.assets[0].data, b"PNG original");
        // Truncated lengths never read past a record or invent an image.
        let mut ctx = Context::new(&options, &token);
        office_art(&art[..art.len() - 1], "Pictures", &mut ctx).unwrap();
        assert!(ctx.assets.is_empty());
        assert!(ctx
            .diagnostics
            .iter()
            .any(|d| d.code == "image_asset_unavailable"));
    }
    #[test]
    #[cfg(feature = "office")]
    fn office_metafiles_use_zlib_and_enforce_declared_expansion_size() {
        use std::io::Write;
        let original = b"original metafile payload";
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(original).unwrap();
        let compressed = compressor.finish().unwrap();
        let mut payload = vec![0u8; 16 + 34];
        payload[16..20].copy_from_slice(&(original.len() as u32).to_le_bytes());
        payload[44..48].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
        payload[49] = 0xfe;
        payload.extend(compressed);
        let options = crate::Options::default();
        let token = crate::CancellationToken::default();
        let mut ctx = Context::new(&options, &token);
        office_art(&record(0x2160, 0xf01b, &payload), "Pictures", &mut ctx).unwrap();
        assert_eq!(&*ctx.assets[0].data, original);
        payload[16..20].copy_from_slice(&1u32.to_le_bytes());
        let mut ctx = Context::new(&options, &token);
        office_art(&record(0x2160, 0xf01b, &payload), "Pictures", &mut ctx).unwrap();
        assert!(ctx.assets.is_empty());
        assert!(ctx
            .diagnostics
            .iter()
            .any(|d| d.code == "image_asset_unavailable"));
    }
}
