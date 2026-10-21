use super::*;

#[test]
fn real_tool_output_plist_is_indented() {
    let source = include_str!("../../../tests/fixtures/structured_text/real-theme.plist");
    let document = Xml.parse(source).expect("complete plist");
    assert_eq!(document.syntax(), "xml");
    assert!(
        document
            .text()
            .contains("\n  <dict>\n    <key>settings</key>")
    );
    assert!(document.text().contains("<string>#123456</string>"));
}

#[test]
fn candidates_require_line_boundaries_and_paired_root() {
    for source in ["use <path> here", "<foo/>", "<p>inline</p> tail"] {
        let start = source.find('<').unwrap();
        assert_eq!(Xml.candidate(source, start), None, "{source}");
    }
    assert_eq!(Xml.candidate("<foo>", 0), Some(5));
    assert!(Xml.parse("<foo>").is_none());
    let source = "before\n<root><child/></root>\nafter";
    assert_eq!(
        Xml.candidate(source, 7),
        Some(source.find("\nafter").unwrap())
    );
}

#[test]
fn invalid_xml_is_rejected_completely() {
    for source in [
        "<root><child></root>",
        "<root>",
        "<root></root><other></other>",
        "<root broken></root>",
        "<root x='1' x='2'></root>",
        "<root x='<bad>'></root>",
        "<1root></1root>",
        "<root>&unknown;</root>",
        "<root>&#x1;</root>",
        "<root><!-- a--b --></root>",
        "<root>bad]]>text</root>",
        "<root x='&unknown;'></root>",
    ] {
        assert!(Xml.parse(source).is_none(), "{source}");
    }
}

#[test]
fn mixed_text_entities_cdata_and_preserved_space_stay_literal() {
    for source in [
        "<root>hello <b>world</b> !</root>",
        "<root>&amp;<b>world</b></root>",
        "<root><![CDATA[x]]><b>world</b></root>",
        "<root xml:space='preserve'><a/><b/></root>",
    ] {
        assert_eq!(Xml.parse(source).unwrap().text(), source);
    }
}

#[test]
fn declaration_fields_and_processing_targets_are_validated() {
    for source in [
        "<?xml version='1.0' broken?><root></root>",
        "<?xml version='1.0' encoding='UTF-8' encoding='UTF-8'?><root></root>",
        "<?xml version='1.0' standalone='yes' standalone='yes'?><root></root>",
        "<?xml version='1.0' encoding='9utf'?><root></root>",
        "<?xml\tversion='1.0' encoding='9utf'?><root></root>",
        "<?xml version='1.0' encoding='UTF 8'?><root></root>",
        "<?xml version='1.0' standalone='maybe'?><root></root>",
        "<?xml version='1.0' standalone='yes' encoding='UTF-8'?><root></root>",
        "<?XML reserved?><root></root>",
        "<?XmL reserved?><root></root>",
        "<?1invalid data?><root></root>",
    ] {
        assert!(Xml.parse(source).is_none(), "{source}");
    }
    for source in [
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><root></root>",
        "<?xml version='1.0' standalone='no'?><root></root>",
        "<?xml-stylesheet type='text/xsl' href='theme.xsl'?><root></root>",
    ] {
        assert!(Xml.parse(source).is_some(), "{source}");
    }
}

#[test]
fn whitespace_only_leaf_values_are_preserved() {
    let source = "<root><a> </a><b>\n  \t</b></root>";
    let formatted = Xml.parse(source).unwrap().text();
    assert!(formatted.contains("<a> </a>"), "{formatted}");
    assert!(formatted.contains("<b>\n  \t</b>"), "{formatted}");
}

#[test]
fn invalid_open_roots_skip_nested_structured_fragments() {
    let source = "<root>\n<child></child>\n{\"ok\": true}";
    assert_eq!(Xml.candidate(source, 0), Some(source.len()));
    assert!(Xml.parse(source).is_none());
    let mismatched = "<root>\n<child></child>\n</wrong>\nafter";
    assert_eq!(
        Xml.candidate(mismatched, 0),
        Some(mismatched.find("\nafter").unwrap())
    );
    assert!(Xml.parse(mismatched).is_none());
}

#[test]
fn namespaces_and_tag_terminators_use_complete_xml_parsing() {
    assert!(
        Xml.parse("<root xmlns:p='urn:a'><p:item/></root>")
            .is_some()
    );
    for source in [
        "<root><p:item/></root>",
        "<root xmlns:p='urn:a' xmlns:q='urn:a' p:x='1' q:x='2'></root>",
    ] {
        assert!(Xml.parse(source).is_none(), "{source}");
    }
    let source = "<root value='>'><!-- </root> --><![CDATA[</root>]]><child/></root>\nafter";
    assert_eq!(
        Xml.candidate(source, 0),
        Some(source.find("\nafter").unwrap())
    );
}

#[test]
fn candidate_scanning_work_is_linear() {
    let source = format!("<root>\n{}\n</root>", "<child value='>'/>\n".repeat(4000));
    SCANNED.with(|count| count.set(0));
    assert_eq!(Xml.candidate(&source, 0), Some(source.len()));
    SCANNED.with(|count| assert!(count.get() <= source.len(), "{}", count.get()));
    let line = "word <path> ".repeat(4000);
    SCANNED.with(|count| count.set(0));
    for (position, _) in line.char_indices() {
        assert!(Xml.candidate(&line, position).is_none());
    }
    SCANNED.with(|count| assert!(count.get() <= line.len(), "{}", count.get()));
}

#[test]
fn mismatched_children_do_not_consume_the_next_block() {
    let source = "<root><child></root>\n{\"ok\":true}";
    assert_eq!(Xml.candidate(source, 0), source.find('\n'));
    let source = "<root><root></root></root>\nafter";
    assert_eq!(Xml.candidate(source, 0), source.find('\n'));
}
