//! General diagnostic taxonomy. Classify the evidence, not the document or corpus.
use crate::{Diagnostic, DiagnosticCategory as Category, DiagnosticEvidence as Evidence, Source};

impl Diagnostic {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        source: Source,
        content_loss: bool,
    ) -> Self {
        let code = code.into();
        let (category, evidence) = classify(&code, content_loss);
        Self {
            code,
            message: message.into(),
            source,
            content_loss,
            category,
            evidence,
        }
    }
}

fn classify(code: &str, legacy_loss: bool) -> (Category, Evidence) {
    use Category::*;
    use Evidence::*;
    match code {
        // A source value is absent/stale, or content is excluded by the declared
        // visible-content policy. These are not parser extraction failures.
        "image_assets_retained"
        | "xlsx_formula_missing_cache"
        | "ods_formula_missing_cache"
        | "xls_formula_missing_cache"
        | "office_chart_missing_cache"
        | "xlsx_error_cell"
        | "docx_fields"
        | "office_hidden_content"
        | "xls_hidden_content"
        | "xlsx_formula_cache"
        | "ods_formula_cache"
        | "sheet_formula_cache"
        | "xlsx_date_normalized"
        | "csv_encoding_inferred"
        | "raw_html"
        | "pptx_alternate"
        | "odf_note"
        | "pdf_unicode_normalized"
        | "pdf_content_spacing_repaired" => (Informational, Detected),

        // The text/value is retained, but structure, styling or placement differs.
        "markdown_table_spans"
        | "markdown_table_headers"
        | "docx_auxiliary_layout"
        | "rtf_auxiliary_layout"
        | "rtf_list"
        | "docx_numbering"
        | "docx_heading_numbering"
        | "doc_auxiliary_layout"
        | "docx_positioned_tab_layout"
        | "docx_list_level_gap"
        | "docx_merge_orphan"
        | "docx_table_style"
        | "docx_styles_unreadable"
        | "docx_numbering_unreadable"
        | "docx_missing_style"
        | "doc_nested_table"
        | "doc_table_cells"
        | "odf_list_numbering"
        | "odf_unmapped"
        | "office_merge_content"
        | "rtf_nested_table"
        | "sheet_number_format"
        | "xlsx_number_format"
        | "xlsx_hyperlink_missing"
        | "pptx_internal_link"
        | "epub_link_target"
        | "epub_duplicate_anchor"
        | "ppt_note_owner"
        | "ppt_master_missing"
        | "ppt_master_style"
        | "ppt_style_metadata"
        | "doc_style_metadata"
        | "doc_table_metadata"
        | "office_math_structure" => (FormattingLimitation, Detected),

        // Broad coverage/style notices are not evidence that a particular source
        // value was omitted. They remain visible, even when text is complete.
        "odf_styles"
        | "xlsx_formatting"
        | "xls_formatting"
        | "epub_css"
        | "office_chart_cache"
        | "xlsx_preview_values"
        | "pdf_approximate_metrics"
        | "pdf_rotated_text"
        | "pdf_vertical_text"
        | "pdf_table_inferred"
        | "pdf_ocr_layout_inferred" => (FormattingLimitation, Unverified),

        // The parser encountered a specific object/data that it did not extract.
        // Images count as content, even when captions or alternative text survive.
        "image_asset_unavailable"
        | "odf_object"
        | "rtf_object"
        | "rtf_binary"
        | "rtf_math"
        | "doc_object"
        | "xls_visual_content"
        | "xls_embedded_substream"
        | "xls_sheet_type"
        | "xls_legacy_comment"
        | "pptx_image"
        | "pptx_graphic"
        | "pptx_shape"
        | "epub_image"
        | "epub_spine_type"
        | "notebook_attachments"
        | "notebook_rich_output"
        | "odf_revisions"
        | "pdf_ocr_figure_unavailable"
        | "pdf_unmapped_characters"
        | "pdf_font_error"
        | "pdf_incomplete_content"
        | "resource_limit_partial"
        | "csv_rows_truncated"
        | "xls_sheets_truncated"
        | "preview_shared_string_limit" => (MissingContent, Detected),

        // May contain unsupported content; presence of a drawing/style/extension
        // alone does not establish that user-visible content was lost.
        "office_math_empty"
        | "doc_formatting"
        | "ppt_layout"
        | "xlsb_formatting"
        | "docx_drawing_layout"
        | "xlsx_visual_content"
        | "pptx_layout_inheritance"
        | "pdf_text_only"
        | "docx_unknown_block"
        | "docx_table_content"
        | "docx_unsupported_inline"
        | "pptx_inline"
        | "docx_auxiliary_missing"
        | "docx_auxiliary_unreadable"
        | "docx_note_missing"
        | "ppt_notes"
        | "pdf_no_text"
        | "pdf_ocr_needed"
        | "pdf_ocr_budget"
        | "pdf_ocr_empty"
        | "pdf_ocr_error"
        | "pdf_ocr_rejected"
        | "pdf_page_error"
        | "pdf_layout_error"
        | "rtf_unbalanced"
        | "pdf_ocr_applied"
        | "pdf_ocr_structure_inferred" => (MissingContent, Unverified),
        // Future diagnostics must never silently become "confirmed loss" or
        // "formatting only" merely because they use the old broad boolean.
        _ if legacy_loss => (MissingContent, Unverified),
        _ => (Informational, Unverified),
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingContent => "missing_content",
            Self::FormattingLimitation => "formatting_limitation",
            Self::Informational => "informational",
        })
    }
}
impl std::fmt::Display for Evidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Detected => "detected",
            Self::Unverified => "unverified",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_omission_formatting_source_conditions_and_uncertainty() {
        for (code, category, evidence) in [
            ("rtf_object", Category::MissingContent, Evidence::Detected),
            (
                "docx_drawing_layout",
                Category::MissingContent,
                Evidence::Unverified,
            ),
            (
                "doc_formatting",
                Category::MissingContent,
                Evidence::Unverified,
            ),
            (
                "docx_auxiliary_layout",
                Category::FormattingLimitation,
                Evidence::Detected,
            ),
            (
                "markdown_table_spans",
                Category::FormattingLimitation,
                Evidence::Detected,
            ),
            (
                "xlsx_formula_missing_cache",
                Category::Informational,
                Evidence::Detected,
            ),
            (
                "xlsx_error_cell",
                Category::Informational,
                Evidence::Detected,
            ),
            (
                "future_warning",
                Category::MissingContent,
                Evidence::Unverified,
            ),
        ] {
            let d = Diagnostic::new(code, "test", Source::default(), true);
            assert_eq!((d.category, d.evidence), (category, evidence));
            assert!(
                d.content_loss,
                "strict-mode compatibility is independent of category"
            );
            let wire = serde_json::to_value(&d).unwrap();
            assert_eq!(serde_json::from_value::<Diagnostic>(wire).unwrap(), d);
        }
    }
}
