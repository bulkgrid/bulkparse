//! BulkParse's own classifier. Uses evidence collected by the existing content
//! walker; never reparses a document, renders a page or invokes OCR.
use super::positioned::Page;
use crate::{
    Context, Error, PdfInspection, PdfInspectionReason as Reason, PdfOcrStatus, PdfPageInspection,
    PdfPageKind as Kind, PdfRoute, SCHEMA_VERSION,
};

/// Fixed-size spatial union: overlapping/repeated images cannot inflate area.
/// Only fully covered grid cells count, conservatively ignoring boundary slivers.
#[derive(Default)]
pub(super) struct ImageCoverage {
    pub page_box: Option<[f64; 4]>,
    rows: [u32; 32],
}
impl ImageCoverage {
    pub fn add(&mut self, rect: [f64; 4]) {
        let Some([x0, y0, x1, y1]) = self.page_box else {
            return;
        };
        let a = (((rect[0] - x0) / (x1 - x0) * 32.).ceil().clamp(0., 32.)) as usize;
        let b = (((rect[2] - x0) / (x1 - x0) * 32.).floor().clamp(0., 32.)) as usize;
        let c = (((rect[1] - y0) / (y1 - y0) * 32.).ceil().clamp(0., 32.)) as usize;
        let d = (((rect[3] - y0) / (y1 - y0) * 32.).floor().clamp(0., 32.)) as usize;
        if a < b && c < d {
            let mask = ((1u64 << b) - (1u64 << a)) as u32;
            for row in &mut self.rows[c..d] {
                *row |= mask;
            }
        }
    }
    pub(super) fn fraction(&self) -> Option<f64> {
        self.page_box
            .map(|_| self.rows.iter().map(|r| r.count_ones()).sum::<u32>() as f64 / 1024.)
    }
}

pub(super) fn suspicious(c: char) -> bool {
    c == '\u{fffd}'
        || (c.is_control() && !c.is_whitespace())
        || matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

// Filled curves are only OCR evidence when they form several text-like lines.
// A large path count alone also describes charts, logos and decorative artwork.
fn outlined_text(page: &Page, characters: usize) -> bool {
    if characters >= 100 || page.outlines.len() < 60.max(characters * 4) {
        return false;
    }
    let Some([x0, y0, x1, y1]) = page.image_coverage.page_box else {
        return false;
    };
    let mut boxes: Vec<_> = page
        .outlines
        .iter()
        .copied()
        .filter(|b| {
            let h = b[3] - b[1];
            let w = b[2] - b[0];
            h >= 3.
                && h <= (y1 - y0) / 12.
                && w >= h * 0.08
                && w <= h * 3.
                && b[0] >= x0
                && b[1] >= y0
                && b[2] <= x1
                && b[3] <= y1
        })
        .collect();
    boxes.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
    let mut lines = 0;
    let mut letters = 0;
    let mut start = 0;
    while start < boxes.len() {
        let first = boxes[start];
        let height = first[3] - first[1];
        let mut end = start + 1;
        while end < boxes.len() && boxes[end][1] - first[1] <= height * 0.2 {
            end += 1;
        }
        let mut row: Vec<_> = boxes[start..end]
            .iter()
            .filter(|b| {
                let h = b[3] - b[1];
                h >= height * 0.6 && h <= height * 1.5
            })
            .collect();
        row.sort_by(|a, b| a[0].total_cmp(&b[0]));
        // Overprinting and nested contours do not count as extra letters.
        let mut count = 0;
        let mut left = 0.;
        let mut right = f64::NEG_INFINITY;
        for b in row {
            if b[0] < right - height * 0.1 {
                continue;
            }
            if b[0] - right > height * 3. {
                if count >= 12 && right - left >= height * 8. {
                    lines += 1;
                    letters += count;
                }
                count = 0;
                left = b[0];
            }
            count += 1;
            right = b[2];
        }
        if count >= 12 && right - left >= height * 8. {
            lines += 1;
            letters += count;
        }
        start = end;
    }
    lines >= 3 && letters >= 60.max(characters * 4)
}

pub(super) fn classify(
    page: &Page,
    number: u32,
    ctx: &Context<'_>,
) -> Result<PdfPageInspection, Error> {
    let mut characters = 0usize;
    let mut bad = 0usize;
    for glyph in &page.glyphs {
        ctx.check()?;
        for c in glyph.text.chars().filter(|c| !c.is_whitespace()) {
            characters += 1;
            bad += usize::from(suspicious(c));
        }
    }
    // Unicode evidence only: no language-specific dictionary, word markers or
    // English letter-frequency assumptions. One damaged glyph in good prose
    // remains a warning rather than replacing an otherwise useful page.
    let broken = bad > 0 && (bad == characters || (bad >= 3 && bad * 5 >= characters));
    let coverage = if page.uncertain_graphics {
        None
    } else {
        page.image_coverage.fraction()
    };
    let has_text = characters > 0 || page.text_operations > 0;
    let kind = match (has_text, page.image_count > 0, page.painted_paths > 0) {
        (true, true, _) => Kind::Mixed,
        (true, false, _) => Kind::Text,
        (false, true, _) => Kind::Image,
        (false, false, true) => Kind::Vector,
        _ if page.uncertain_graphics => Kind::Unknown,
        _ => Kind::Blank,
    };
    let mut reasons = Vec::new();
    if page.invisible_text_operations > 0 {
        reasons.push(Reason::InvisibleTextLayer);
    }
    if page.uncertain_graphics {
        reasons.push(Reason::UncertainGraphics);
    }
    if bad > 0 {
        reasons.push(Reason::SuspectTextEncoding);
    }
    if page.undecodable_text {
        reasons.push(Reason::UndecodableText);
    }
    let route = if page.incomplete_content {
        reasons.push(Reason::ExtractionFailed);
        PdfRoute::Review
    } else if broken || page.undecodable_text {
        PdfRoute::Ocr
    } else if characters == 0 && kind != Kind::Blank {
        reasons.push(Reason::NoNativeText);
        if page.painted_paths > 0 {
            reasons.push(Reason::VectorContent);
        }
        if kind == Kind::Unknown {
            PdfRoute::Review
        } else {
            PdfRoute::Ocr
        }
    } else if coverage.is_some_and(|area| area >= 0.5) && characters < 100 {
        // A masthead, page number or stamp must not hide a large scanned body.
        reasons.push(Reason::LargeImageWithSparseText);
        PdfRoute::Ocr
    } else if outlined_text(page, characters) {
        reasons.push(Reason::OutlinedTextWithSparseNativeText);
        reasons.push(Reason::VectorContent);
        PdfRoute::Ocr
    } else if kind == Kind::Blank {
        reasons.push(Reason::NoPaintedContent);
        PdfRoute::Skip
    } else {
        reasons.push(Reason::UsableNativeText);
        PdfRoute::Native
    };
    Ok(PdfPageInspection {
        page: number,
        kind,
        route,
        reasons,
        native_characters: characters,
        suspicious_characters: bad,
        text_operations: page.text_operations,
        invisible_text_operations: page.invisible_text_operations,
        image_count: page.image_count,
        image_coverage: coverage,
        painted_paths: page.painted_paths,
        ocr_status: if matches!(route, PdfRoute::Ocr | PdfRoute::Review) {
            PdfOcrStatus::NotRequested
        } else {
            PdfOcrStatus::NotNeeded
        },
    })
}

pub(super) fn failed(page: u32) -> PdfPageInspection {
    PdfPageInspection {
        page,
        kind: Kind::Unknown,
        route: PdfRoute::Review,
        reasons: vec![Reason::ExtractionFailed],
        native_characters: 0,
        suspicious_characters: 0,
        text_operations: 0,
        invisible_text_operations: 0,
        image_count: 0,
        image_coverage: None,
        painted_paths: 0,
        ocr_status: PdfOcrStatus::NotRequested,
    }
}

pub(super) fn report(pages: Vec<PdfPageInspection>) -> PdfInspection {
    let mut kind = Kind::Blank;
    for page in &pages {
        if page.kind == Kind::Unknown {
            kind = Kind::Unknown;
            break;
        }
        if page.kind != Kind::Blank {
            kind = if kind == Kind::Blank || kind == page.kind {
                page.kind
            } else {
                Kind::Mixed
            };
        }
    }
    PdfInspection {
        schema_version: SCHEMA_VERSION.into(),
        kind,
        page_count: pages.len(),
        pages_needing_ocr: pages
            .iter()
            .filter(|p| p.route == PdfRoute::Ocr)
            .map(|p| p.page)
            .collect(),
        pages,
    }
}
