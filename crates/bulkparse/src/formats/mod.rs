#[cfg(feature = "office")]
pub(super) mod bounded;
#[cfg(feature = "docx")]
mod docx;
mod markdown;
mod notebook;
#[cfg(feature = "office")]
mod office;
#[cfg(feature = "pdf")]
pub(crate) mod pdf;
mod tabular;
pub(crate) mod text;
use crate::{Block, Context, Error, Format};

pub(crate) fn write(
    format: Format,
    bytes: &[u8],
    ctx: &mut Context<'_>,
    output: &mut crate::render::Writer<'_>,
    recovery: bool,
) -> Result<(), Error> {
    if matches!(format, Format::Csv | Format::Tsv) {
        if recovery {
            tabular::preview_output(bytes, format, ctx, Some(output))?;
        } else {
            tabular::parse_output(bytes, format, ctx, Some(output))?;
        }
        return Ok(());
    }
    #[cfg(feature = "office")]
    if format == Format::Xls {
        return office::write(format, bytes, ctx, output);
    }
    if recovery {
        #[cfg(feature = "office")]
        if matches!(format, Format::Xlsx | Format::Ods) {
            office::preview::parse_output(format, bytes, ctx, Some(output))?;
            return Ok(());
        }
        output.blocks(&recover(format, bytes, ctx)?)?;
        return Ok(());
    }
    #[cfg(feature = "office")]
    if matches!(format, Format::Xlsx | Format::Xlsb | Format::Ods) {
        return office::write(format, bytes, ctx, output);
    }
    output.document_blocks(format, &parse(format, bytes, ctx)?)?;
    Ok(())
}

pub fn detect(
    bytes: &[u8],
    filename: Option<&str>,
    explicit: Option<Format>,
    ctx: &mut Context<'_>,
) -> Result<Format, Error> {
    #[cfg(feature = "office")]
    let office_signature = office::detect(bytes, ctx)?;
    #[cfg(not(feature = "office"))]
    let office_signature = {
        let _ = ctx;
        None
    };
    let signature = if bytes.starts_with(b"%PDF-") {
        Some(Format::Pdf)
    } else if crate::is_image(bytes) {
        Some(Format::Image)
    } else if office_signature.is_some() {
        office_signature
    } else if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        #[cfg(feature = "docx")]
        {
            if docx::is_docx(bytes)? {
                Some(Format::Docx)
            } else {
                return Err(Error::Unsupported("unrecognized document package".into()));
            }
        }
        #[cfg(not(feature = "docx"))]
        {
            return Err(Error::FeatureDisabled("docx"));
        }
    } else {
        None
    };
    if let Some(actual) = signature {
        if explicit.is_some_and(|f| f != actual) {
            return Err(Error::Malformed(
                "explicit format conflicts with file signature".into(),
            ));
        }
        return Ok(actual);
    }
    let selected = explicit.or_else(|| {
        filename
            .and_then(|n| n.rsplit('.').next())
            .and_then(Format::from_name)
    });
    match selected {
        Some(
            Format::Pdf
            | Format::Image
            | Format::Docx
            | Format::Xlsx
            | Format::Xlsb
            | Format::Pptx
            | Format::Odt
            | Format::Ods
            | Format::Odp
            | Format::Epub
            | Format::Doc
            | Format::Ppt
            | Format::Xls
            | Format::Rtf,
        ) => Err(Error::Malformed("missing document signature".into())),
        Some(format) => Ok(format),
        None => Err(Error::Unsupported(
            "provide a format or filename for signature-less inputs".into(),
        )),
    }
}

pub fn utf8(bytes: &[u8]) -> Result<&str, Error> {
    std::str::from_utf8(bytes)
        .map(|s| s.strip_prefix('\u{feff}').unwrap_or(s))
        .map_err(|_| Error::Malformed("expected UTF-8 text".into()))
}

pub fn recover(format: Format, bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    match format {
        Format::Csv | Format::Tsv => tabular::preview(bytes, format, ctx),
        #[cfg(feature = "office")]
        Format::Xlsx | Format::Ods => office::preview::parse(format, bytes, ctx),
        #[cfg(feature = "office")]
        Format::Xls => office::parse(format, bytes, ctx),
        _ => Err(Error::Unsupported(
            "bounded recovery is supported for CSV/TSV, XLS, XLSX/XLSM and ODS only".into(),
        )),
    }
}

pub fn parse(format: Format, bytes: &[u8], ctx: &mut Context<'_>) -> Result<Vec<Block>, Error> {
    match format {
        Format::Image => Err(Error::Unsupported(
            "image input requires an OCR provider".into(),
        )),
        Format::Text => text::parse(utf8(bytes)?, ctx, Default::default()),
        Format::Markdown => markdown::parse(utf8(bytes)?, ctx, Default::default()),
        Format::Csv | Format::Tsv => tabular::parse(bytes, format, ctx),
        Format::Notebook => notebook::parse(bytes, ctx),
        Format::Docx => {
            #[cfg(feature = "docx")]
            {
                docx::parse(bytes, ctx)
            }
            #[cfg(not(feature = "docx"))]
            {
                Err(Error::FeatureDisabled("docx"))
            }
        }
        Format::Xlsx
        | Format::Xls
        | Format::Xlsb
        | Format::Pptx
        | Format::Ppt
        | Format::Odt
        | Format::Ods
        | Format::Odp
        | Format::Epub
        | Format::Rtf
        | Format::Doc => {
            #[cfg(feature = "office")]
            {
                office::parse(format, bytes, ctx)
            }
            #[cfg(not(feature = "office"))]
            {
                Err(Error::FeatureDisabled("office"))
            }
        }
        Format::Pdf => {
            #[cfg(feature = "pdf")]
            {
                pdf::parse(bytes, ctx)
            }
            #[cfg(not(feature = "pdf"))]
            {
                Err(Error::FeatureDisabled("pdf"))
            }
        }
    }
}
