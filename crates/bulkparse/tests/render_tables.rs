use bulkparse::{Block, BlockContent, Cell, Inline, Parser, Source};

fn block(content: BlockContent) -> Block {
    Block {
        id: String::new(),
        source: Source::default(),
        content,
    }
}
fn paragraph(text: &str) -> Block {
    block(BlockContent::Paragraph {
        inlines: vec![Inline::text(text)],
    })
}
fn cell(text: &str, row_span: u32, col_span: u32) -> Cell {
    Cell {
        blocks: vec![paragraph(text)],
        row_span,
        col_span,
        source: Source::default(),
    }
}
fn table(rows: Vec<Vec<Cell>>, header_rows: usize) -> Block {
    block(BlockContent::Table { rows, header_rows })
}
fn render(blocks: Vec<Block>) -> bulkparse::MarkdownOutput {
    let mut parsed = Parser::default()
        .parse_bytes(b"text", Some("input.txt"))
        .unwrap();
    parsed.document.blocks = blocks;
    parsed.to_markdown()
}
fn grid(markdown: &str) -> Vec<Vec<String>> {
    use pulldown_cmark::{Event, Options, Tag, TagEnd};
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut in_cell = false;
    for event in pulldown_cmark::Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    ) {
        match event {
            Event::Start(Tag::TableHead | Tag::TableRow) => rows.push(Vec::new()),
            Event::Start(Tag::TableCell) => {
                rows.last_mut().unwrap().push(String::new());
                in_cell = true;
            }
            Event::End(TagEnd::TableCell) => in_cell = false,
            Event::Text(text) | Event::Code(text) if in_cell => {
                rows.last_mut().unwrap().last_mut().unwrap().push_str(&text);
            }
            Event::InlineHtml(html) if html.as_ref() == "<br>" && in_cell => {
                rows.last_mut().unwrap().last_mut().unwrap().push('\n');
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                assert!(
                    !html.contains("<table") && !html.contains("<tr") && !html.contains("<td"),
                    "{html}"
                );
            }
            _ => {}
        }
    }
    rows
}

#[test]
fn headerless_tables_keep_the_first_record_as_data_for_any_source() {
    for page in [None, Some(1)] {
        for part in [None, Some("content.xml".into())] {
            let mut t = table(vec![vec![cell("Almonds", 1, 1), cell("50 kr", 1, 1)]], 0);
            t.source = Source {
                page,
                part,
                ..Default::default()
            };
            let output = render(vec![t]);
            assert_eq!(output.text, "|  |  |\n| --- | --- |\n| Almonds | 50 kr |\n");
            assert_eq!(grid(&output.text), [vec!["", ""], vec!["Almonds", "50 kr"]]);
        }
    }
}

#[test]
fn merged_cells_keep_columns_without_requiring_provenance() {
    let t = table(
        vec![
            vec![cell("Group", 3, 2), cell("Heading", 1, 2)],
            vec![cell("B", 1, 1), cell("C", 2, 1)],
            vec![cell("D", 1, 1)],
            vec![cell("E", 1, 1), cell("F", 1, 2), cell("G", 1, 1)],
        ],
        0,
    );
    let output = render(vec![t.clone()]);
    assert_eq!(
        grid(&output.text),
        [
            vec!["", "", "", ""],
            vec!["Group", "", "Heading", ""],
            vec!["", "", "B", "C"],
            vec!["", "", "D", ""],
            vec!["E", "F", "", "G"],
        ]
    );
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(output.diagnostics[0].code, "markdown_table_spans");
    // Tables inside quotes use precisely the same renderer and placement.
    let quoted = render(vec![block(BlockContent::Quote { blocks: vec![t] })]);
    assert_eq!(grid(&quoted.text), grid(&output.text));
    assert_eq!(quoted.diagnostics, output.diagnostics);
}

#[test]
fn multiple_headers_and_empty_covered_rows_keep_all_text() {
    let output = render(vec![table(
        vec![
            vec![cell("First", 2, 2)],
            vec![],
            vec![cell("Final", 1, 1), cell("value", 1, 1)],
        ],
        2,
    )]);
    assert_eq!(
        grid(&output.text),
        [vec!["First", ""], vec!["", ""], vec!["Final", "value"]]
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["markdown_table_spans", "markdown_table_headers"]
    );
}

#[test]
fn nested_tables_lists_and_code_remain_in_their_parent_cell() {
    let mut rich = cell("Intro", 1, 1);
    rich.blocks.push(block(BlockContent::List {
        start: Some(3),
        items: vec![vec![paragraph("A | B")], vec![paragraph("<tag> &amp;")]],
    }));
    rich.blocks.push(table(
        vec![
            vec![cell("Nested", 1, 2), cell("Tail", 1, 1)],
            vec![cell("x", 1, 1), cell("y", 1, 1), cell("z", 1, 1)],
        ],
        0,
    ));
    rich.blocks.push(block(BlockContent::Code {
        language: "rust".into(),
        text: "a | `b`".into(),
    }));
    let output = render(vec![table(vec![vec![rich, cell("Neighbor", 1, 1)]], 0)]);
    let rows = grid(&output.text);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].len(), 2);
    assert_eq!(rows[1][1], "Neighbor");
    assert!(
        rows[1][0].contains("Intro\n3. A | B\n4. <tag> &amp;\nNested /  / Tail\nx / y / z\n"),
        "{:?}",
        rows[1]
    );
    assert!(rows[1][0].ends_with("a | `b`"), "{:?}", rows[1]);
    assert!(!output.text.contains("<table>"));
}

#[test]
fn pdf_fragments_join_with_spaces_but_authored_paragraphs_keep_breaks() {
    for (page, part, expected) in [
        (Some(1), None, "first second third"),
        (Some(1), Some("slides.xml".into()), "first\nsecond\nthird"),
        (None, None, "first\nsecond\nthird"),
    ] {
        let mut c = cell("first", 1, 1);
        c.blocks.push(block(BlockContent::Paragraph {
            inlines: vec![Inline::text("second"), Inline::Break, Inline::text("third")],
        }));
        let mut t = table(vec![vec![c]], 0);
        t.source = Source {
            page,
            part,
            ..Default::default()
        };
        assert_eq!(grid(&render(vec![t]).text)[1][0], expected);
    }
}

#[test]
fn code_and_literal_pipes_round_trip_without_creating_columns() {
    for value in ["a | b", "`a` | b", r"a \| b", r"a \\| b", "a || b"] {
        let mut c = cell("", 1, 1);
        c.blocks[0] = block(BlockContent::Paragraph {
            inlines: vec![Inline::Code { text: value.into() }],
        });
        let output = render(vec![table(vec![vec![c, cell(value, 1, 1)]], 0)]);
        assert_eq!(grid(&output.text)[1], [value, value], "{}", output.text);
    }
}

#[test]
fn quantities_and_literal_strikethrough_markers_survive_both_cell_paths() {
    for value in [
        "~350 bottles",
        "~a~",
        "~~a~~",
        "~~",
        "~~~",
        "~ a ~",
        "a~b~~c",
    ] {
        let plain = cell(value, 1, 1);
        let mut rich = plain.clone();
        let BlockContent::Paragraph { inlines } = &mut rich.blocks[0].content else {
            unreachable!()
        };
        inlines.push(Inline::text(""));
        let output = render(vec![table(vec![vec![plain, rich]], 0)]);
        assert_eq!(grid(&output.text)[1], [value, value], "{}", output.text);
        if value == "~350 bottles" {
            assert!(!output.text.contains('\\'));
        }
    }
}

#[test]
fn adjacent_styled_fragments_share_cell_break_rules() {
    for (page, expected) in [(None, "a\nbc\nd"), (Some(1), "a bc d")] {
        let mut c = cell("", 1, 1);
        c.blocks[0] = block(BlockContent::Paragraph {
            inlines: vec![
                Inline::Strong {
                    children: vec![Inline::text("a"), Inline::Break, Inline::text("b")],
                },
                Inline::Strong {
                    children: vec![Inline::text("c"), Inline::Break, Inline::text("d")],
                },
            ],
        });
        let mut t = table(vec![vec![c]], 0);
        t.source.page = page;
        assert_eq!(grid(&render(vec![t]).text)[1][0], expected);
    }
}
