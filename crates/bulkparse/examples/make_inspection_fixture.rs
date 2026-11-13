//! Reproducible integration fixture: add native text to an authored scan.
//! cargo run -p bulkparse --example make_inspection_fixture --offline
#[cfg(feature = "pdf")]
fn main() {
    use lopdf::{dictionary, Document, Object, Stream};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut doc = Document::load(root.join("fixtures/ocr/ocr-prose.pdf")).unwrap();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let page = doc.get_pages()[&1];
    let resources = doc.get_page_resources(page).unwrap();
    let mut resources = resources
        .0
        .cloned()
        .or_else(|| {
            resources
                .1
                .first()
                .and_then(|id| doc.get_dictionary(*id).ok().cloned())
        })
        .unwrap();
    resources.set("Font", dictionary! {"InspectorHeader"=>font});
    let original = doc.get_page_content(page).unwrap();
    let mut content = original;
    content.extend_from_slice(b"\nBT /InspectorHeader 12 Tf 40 760 Td (Monthly archive) Tj ET\n");
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    let page = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    page.set("Resources", Object::Dictionary(resources));
    page.set("Contents", stream);
    let directory = root.join("fixtures/inspection");
    std::fs::create_dir_all(&directory).unwrap();
    doc.save(directory.join("header-over-scan.pdf")).unwrap();
}
#[cfg(not(feature = "pdf"))]
fn main() {}
