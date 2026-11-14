use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// All wire formats use the same schema. Changes to it require a version bump.
pub const SCHEMA_VERSION: &str = "0.8";

/// Semantic OCR region. Bounds are in rendered-page pixels; order is supplied
/// by the layout provider. Missing confidence stays unknown, never fabricated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfRegionSource {
    pub image_size: [u32; 2],
    pub bbox: [f64; 4],
    pub order: u32,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// OCR evidence in rendered-page pixels, origin at top left, x right and y down.
/// Quad vertices follow the provider's perimeter order; confidence is an engine
/// score in [0, 1], not a calibrated probability of correctness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OcrSource {
    pub image_size: [u32; 2],
    pub quad: [[f64; 2]; 4],
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Text,
    Markdown,
    Csv,
    Tsv,
    Notebook,
    Docx,
    Pdf,
    Image,
    Xlsx,
    Xls,
    Xlsb,
    Pptx,
    Ppt,
    Odt,
    Ods,
    Odp,
    Epub,
    Rtf,
    Doc,
}

impl Format {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "txt" | "text" => Some(Self::Text),
            "md" | "markdown" => Some(Self::Markdown),
            "csv" => Some(Self::Csv),
            "tsv" => Some(Self::Tsv),
            "ipynb" | "notebook" => Some(Self::Notebook),
            "docx" | "docm" => Some(Self::Docx),
            "pdf" => Some(Self::Pdf),
            "image" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "tif" | "tiff" => Some(Self::Image),
            "xlsx" | "xlsm" => Some(Self::Xlsx),
            "xls" => Some(Self::Xls),
            "xlsb" => Some(Self::Xlsb),
            "pptx" | "pptm" | "ppsx" | "ppsm" => Some(Self::Pptx),
            "ppt" | "pps" | "pot" => Some(Self::Ppt),
            "odt" => Some(Self::Odt),
            "ods" => Some(Self::Ods),
            "odp" => Some(Self::Odp),
            "epub" => Some(Self::Epub),
            "rtf" => Some(Self::Rtf),
            "doc" => Some(Self::Doc),
            _ => None,
        }
    }
}

/// Page, cell and row coordinates are one-based. Byte ranges are zero-based,
/// UTF-8, end-exclusive offsets into the decoded source text/part/cell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_region: Option<Box<PdfRegionSource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr: Option<Box<OcrSource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Shared worksheet/package identifier; serialized as the same JSON string.
    /// Cloning cell provenance must not allocate another copy per paragraph.
    #[serde(default, deserialize_with = "deserialize_part")]
    pub part: Option<std::sync::Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    // Most cell sources have no byte range. Keep the uncommon pair out of the
    // per-cell layout, sharing it when a located source is cloned. Serde and
    // JsonSchema expose the same optional two-integer array.
    pub byte_range: Option<std::sync::Arc<[usize; 2]>>,
}

/// An original embedded image file. Position is not inferred from asset order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImageAsset {
    /// Content-derived, safe filename, including extension.
    pub id: String,
    pub media_type: String,
    pub source: Source,
    #[serde(with = "crate::assets::base64_bytes")]
    #[schemars(with = "String")]
    pub data: std::sync::Arc<[u8]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Document {
    pub schema_version: String,
    pub format: Format,
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub assets: Vec<ImageAsset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Block {
    pub id: String,
    pub source: Source,
    #[serde(flatten)]
    pub content: BlockContent,
}

// Typed fields decode nested tables directly instead of buffering an entire
// worksheet in serde's generic flatten/internally-tagged representation. Field
// order remains arbitrary and the serialized contract is unchanged.
impl<'de> Deserialize<'de> for Block {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            source: Source,
            #[serde(rename = "type")]
            kind: String,
            #[serde(default, deserialize_with = "compact_optional_vec")]
            inlines: Option<Vec<Inline>>,
            level: Option<u8>,
            language: Option<String>,
            text: Option<String>,
            blocks: Option<Vec<Block>>,
            start: Option<u64>,
            items: Option<Vec<Vec<Block>>>,
            rows: Option<Vec<Vec<Cell>>>,
            header_rows: Option<usize>,
            media_type: Option<String>,
        }
        fn required<T, E: serde::de::Error>(v: Option<T>, name: &'static str) -> Result<T, E> {
            v.ok_or_else(|| E::missing_field(name))
        }
        let w = Wire::deserialize(d)?;
        let content = match w.kind.as_str() {
            "paragraph" => BlockContent::Paragraph {
                inlines: required(w.inlines, "inlines")?,
            },
            "heading" => BlockContent::Heading {
                level: required(w.level, "level")?,
                inlines: required(w.inlines, "inlines")?,
            },
            "code" => BlockContent::Code {
                language: required(w.language, "language")?,
                text: required(w.text, "text")?,
            },
            "quote" => BlockContent::Quote {
                blocks: required(w.blocks, "blocks")?,
            },
            "list" => BlockContent::List {
                start: w.start,
                items: required(w.items, "items")?,
            },
            "table" => BlockContent::Table {
                rows: required(w.rows, "rows")?,
                header_rows: required(w.header_rows, "header_rows")?,
            },
            "thematic_break" => BlockContent::ThematicBreak,
            "raw" => BlockContent::Raw {
                media_type: required(w.media_type, "media_type")?,
                text: required(w.text, "text")?,
            },
            other => {
                return Err(serde::de::Error::unknown_variant(
                    other,
                    &[
                        "paragraph",
                        "heading",
                        "code",
                        "quote",
                        "list",
                        "table",
                        "thematic_break",
                        "raw",
                    ],
                ))
            }
        };
        Ok(Self {
            id: w.id,
            source: w.source,
            content,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockContent {
    Paragraph {
        #[serde(deserialize_with = "compact_vec")]
        inlines: Vec<Inline>,
    },
    Heading {
        level: u8,
        #[serde(deserialize_with = "compact_vec")]
        inlines: Vec<Inline>,
    },
    Code {
        language: String,
        text: String,
    },
    Quote {
        blocks: Vec<Block>,
    },
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Table {
        rows: Vec<Vec<Cell>>,
        header_rows: usize,
    },
    ThematicBreak,
    /// Preserved evidence that has no semantic mapping yet. Rendered as code.
    Raw {
        media_type: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Cell {
    #[serde(deserialize_with = "compact_vec")]
    pub blocks: Vec<Block>,
    pub col_span: u32,
    pub row_span: u32,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    /// Generated TeX from a structured equation; no source commands are executed.
    Math {
        tex: String,
        display: bool,
    },
    /// Stable target for document-internal links.
    Anchor {
        id: String,
    },
    Text {
        text: String,
    },
    Strong {
        #[serde(deserialize_with = "compact_vec")]
        children: Vec<Inline>,
    },
    Emphasis {
        #[serde(deserialize_with = "compact_vec")]
        children: Vec<Inline>,
    },
    Strike {
        #[serde(deserialize_with = "compact_vec")]
        children: Vec<Inline>,
    },
    Code {
        text: String,
    },
    Link {
        url: String,
        #[serde(deserialize_with = "compact_vec")]
        children: Vec<Inline>,
    },
    Image {
        url: String,
        alt: String,
    },
    Break,
}

impl Inline {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Complete,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub source: Source,
    /// Legacy broad fidelity flag, including formatting and unverifiable coverage.
    /// Preserved for strict-mode compatibility; use category/evidence for omissions.
    pub content_loss: bool,
    pub category: DiagnosticCategory,
    pub evidence: DiagnosticEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    MissingContent,
    FormattingLimitation,
    Informational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticEvidence {
    /// A specific unsupported source object or condition was detected.
    Detected,
    /// A capability warning or uncertain condition, not proof of an omission.
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Coverage {
    pub source: Source,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ParseResult {
    /// Present for PDFs. Recommendations describe native evidence, independently
    /// of whether an optional OCR provider was configured or succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_inspection: Option<PdfInspection>,
    pub document: Document,
    pub status: Status,
    pub diagnostics: Vec<Diagnostic>,
    pub coverage: Vec<Coverage>,
    pub parser_version: String,
}

impl ParseResult {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    /// Rendering has its own diagnostics (e.g. complex tables in GFM).
    pub fn to_markdown(&self) -> crate::MarkdownOutput {
        crate::render::render(&self.document)
    }
}

/// Structural classification, not a claim that an image contains text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfPageKind {
    Text,
    Image,
    Mixed,
    Vector,
    Blank,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfRoute {
    Native,
    Ocr,
    Skip,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfInspectionReason {
    UsableNativeText,
    NoNativeText,
    SuspectTextEncoding,
    UndecodableText,
    LargeImageWithSparseText,
    OutlinedTextWithSparseNativeText,
    InvisibleTextLayer,
    VectorContent,
    NoPaintedContent,
    UncertainGraphics,
    ExtractionFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PdfOcrStatus {
    NotNeeded,
    NotRequested,
    Applied,
    Empty,
    Failed,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfPageInspection {
    pub page: u32,
    pub kind: PdfPageKind,
    pub route: PdfRoute,
    pub reasons: Vec<PdfInspectionReason>,
    pub native_characters: usize,
    pub suspicious_characters: usize,
    pub text_operations: usize,
    pub invisible_text_operations: usize,
    pub image_count: usize,
    /// Approximate union of painted image bounds in the page box (0..1).
    /// None when geometry/graphics cannot be interpreted conservatively.
    pub image_coverage: Option<f64>,
    pub painted_paths: usize,
    pub ocr_status: PdfOcrStatus,
}

/// Every page is inspected. No sampling, rendering, inference or network access
/// is performed by the inspection-only API. Scores are deliberately not
/// presented as calibrated confidence probabilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PdfInspection {
    pub schema_version: String,
    pub kind: PdfPageKind,
    pub page_count: usize,
    pub pages: Vec<PdfPageInspection>,
    pub pages_needing_ocr: Vec<u32>,
}

// JSON arrays do not supply a length hint. Serde's ordinary Vec growth reserves
// four large elements for one-element cells/inlines. Start these small semantic
// arrays at one, without changing wire representation or unknown-field behavior.
fn compact_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let Some(first) = seq.next_element()? else {
                return Ok(Vec::new());
            };
            let mut values =
                Vec::with_capacity(seq.size_hint().unwrap_or(0).saturating_add(1).min(16));
            values.push(first);
            while let Some(value) = seq.next_element()? {
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Visitor(std::marker::PhantomData))
}

fn deserialize_part<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<std::sync::Arc<str>>, D::Error> {
    use std::sync::{Arc, Weak};
    thread_local! {
        // Weak references never retain an old document. Key storage is bounded:
        // at most 256 names of at most 4096 bytes, independent of document size.
        static PARTS: std::cell::RefCell<std::collections::HashMap<String, Weak<str>>> = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    struct Part;
    impl<'de> serde::de::Visitor<'de> for Part {
        type Value = Arc<str>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a part name")
        }
        fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Arc<str>, E> {
            if text.len() > 4096 {
                return Ok(Arc::from(text));
            }
            Ok(PARTS.with(|parts| {
                let mut parts = parts.borrow_mut();
                if let Some(part) = parts.get(text).and_then(Weak::upgrade) {
                    return part;
                }
                if parts.len() >= 256 {
                    parts.clear();
                }
                let part: Arc<str> = Arc::from(text);
                parts.insert(text.to_owned(), Arc::downgrade(&part));
                part
            }))
        }
    }
    struct Optional;
    impl<'de> serde::de::Visitor<'de> for Optional {
        type Value = Option<Arc<str>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a part name or null")
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_str(Part).map(Some)
        }
    }
    d.deserialize_option(Optional)
}

fn compact_optional_vec<'de, D, T>(d: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Visitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<T> {
        type Value = Option<Vec<T>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array or null")
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            compact_vec(d).map(Some)
        }
    }
    d.deserialize_option(Visitor(std::marker::PhantomData))
}

#[cfg(test)]
mod compact_decode_tests {
    use super::*;
    #[test]
    fn byte_ranges_preserve_wire_values_and_share_cloned_storage() {
        let source = Source {
            byte_range: Some([0, usize::MAX].into()),
            ..Default::default()
        };
        let cloned = source.clone();
        assert!(std::sync::Arc::ptr_eq(
            source.byte_range.as_ref().unwrap(),
            cloned.byte_range.as_ref().unwrap(),
        ));
        let value = serde_json::to_value(&source).unwrap();
        assert_eq!(value, serde_json::json!({"byte_range": [0, usize::MAX]}));
        assert_eq!(serde_json::from_value::<Source>(value).unwrap(), source);
        for json in ["{}", r#"{"byte_range":null}"#] {
            let empty: Source = serde_json::from_str(json).unwrap();
            assert!(empty.byte_range.is_none());
            assert_eq!(serde_json::to_value(empty).unwrap(), serde_json::json!({}));
        }
        for json in [r#"{"byte_range":[0]}"#, r#"{"byte_range":[0,1,2]}"#] {
            assert!(serde_json::from_str::<Source>(json).is_err());
        }
    }
    #[test]
    fn decoded_cells_share_provenance_and_keep_single_element_arrays_small() {
        let cell = CELL;
        let cells: Vec<Cell> = serde_json::from_str(&format!("[{cell},{cell}]")).unwrap();
        let part = cells[0].source.part.as_ref().unwrap();
        for cell in &cells {
            assert_eq!(cell.blocks.capacity(), 1);
            assert!(std::sync::Arc::ptr_eq(
                part,
                cell.source.part.as_ref().unwrap()
            ));
            assert!(std::sync::Arc::ptr_eq(
                part,
                cell.blocks[0].source.part.as_ref().unwrap()
            ));
            if let BlockContent::Paragraph { inlines } = &cell.blocks[0].content {
                assert_eq!(inlines.capacity(), 1);
            }
            assert_eq!(
                serde_json::to_value(cell).unwrap(),
                serde_json::from_str::<serde_json::Value>(CELL).unwrap()
            );
        }
        assert!(serde_json::from_str::<Source>("{}").unwrap().part.is_none());
        assert!(serde_json::from_str::<Source>(r#"{"part":null}"#)
            .unwrap()
            .part
            .is_none());
    }
    const CELL: &str = r#"{"blocks":[{"id":"b1","source":{"part":"xl/test.xml","row":1,"cell":1},"type":"paragraph","inlines":[{"type":"text","text":"value"}]}],"col_span":1,"row_span":1,"source":{"part":"xl/test.xml","row":1,"cell":1}}"#;
}
