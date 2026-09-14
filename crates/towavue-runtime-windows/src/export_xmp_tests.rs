use super::*;

#[test]
fn current_document_metadata_rejects_unresolved_ancestor_bases() {
    let cancel = AtomicBool::new(false);
    let original = format!(
        "<x:xmpmeta xmlns:x=\"{META}\"><r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:m=\"{DM}\" r:about=\"\"><m:genre>kept</m:genre></r:Description></r:RDF></x:xmpmeta>"
    );
    for element in ["x:xmpmeta", "r:RDF", "r:Description"] {
        for base in [
            "https://example.invalid/other",
            "../other.jpg",
            "https://example.invalid/&#111;ther",
        ] {
            let packet = original.replace(
                &format!("<{element} "),
                &format!("<{element} xml:base=\"{base}\" "),
            );
            assert!(
                parse(packet.as_bytes(), &cancel).is_err(),
                "unresolved base accepted on {element}"
            );
            for value in [None, Some("changed"), Some("")] {
                let mut options = MetadataExportOptions::default();
                options
                    .set(MetadataField::Genre, value.map(str::to_owned))
                    .expect("option");
                assert!(rewrite_unedited(packet.as_bytes(), &options, &cancel).is_err());
                assert!(rewrite_edited(packet.as_bytes(), &options, &cancel).is_err());
            }
        }
        let neutral = original.replace(
            &format!("<{element} "),
            &format!("<{element} xml:base=\"\" "),
        );
        assert_eq!(
            rewrite_unedited(
                neutral.as_bytes(),
                &MetadataExportOptions::default(),
                &cancel
            )
            .expect("empty base"),
            neutral.as_bytes()
        );
    }
    // Unlike xml:lang, an empty child base does not undo an ancestor's base URI.
    let inherited = original
        .replace("<r:RDF ", "<r:RDF xml:base=\"../other.jpg\" ")
        .replace("<r:Description ", "<r:Description xml:base=\"\" ");
    assert!(parse(inherited.as_bytes(), &cancel).is_err());
    // Opaque property-local context is retained, not interpreted as the image subject.
    let rights = original.replace("</r:Description>", &format!("<q:WebStatement xmlns:q=\"{RIGHTS}\" xml:base=\"https://example.invalid/\">rights</q:WebStatement></r:Description>"));
    let kept = rewrite_edited(
        rights.as_bytes(),
        &MetadataExportOptions::default(),
        &cancel,
    )
    .expect("property-local base");
    assert!(
        std::str::from_utf8(&kept)
            .expect("XML")
            .contains("xml:base=\"https://example.invalid/\">rights")
    );
}

#[test]
fn rewritten_values_do_not_inherit_an_ancestor_language() {
    fn languages(node: &Node, inherited: &str, output: &mut Vec<(String, String)>) {
        let language = node
            .attributes
            .iter()
            .find(|(key, _)| key.is(XML, "lang"))
            .map_or(inherited, |(_, value)| value);
        if node.children.is_empty() && !node.text.trim().is_empty() {
            output.push((node.text.clone(), language.into()));
        }
        for child in &node.children {
            languages(child, language, output);
        }
    }
    let cancel = AtomicBool::new(false);
    for (wrapper, rdf_language, description_language, expected_owner_language) in [
        (false, " xml:lang=\"fr\"", "", "fr"),
        (true, "", "", "fr"),
        (true, " xml:lang=\"\"", "", ""),
        (true, "", " xml:lang=\"\"", ""),
        (true, "", " xml:lang=\"de\"", "de"),
    ] {
        let rdf = format!(
            "<r:RDF xmlns:r=\"{RDF}\"{rdf_language}><r:Description xmlns:q=\"{RIGHTS}\"{description_language}><q:Owner><r:Bag><r:li>kept owner</r:li></r:Bag></q:Owner></r:Description></r:RDF>"
        );
        let original = if wrapper {
            format!("<x:xmpmeta xmlns:x=\"{META}\" xml:lang=\"fr\">{rdf}</x:xmpmeta>")
        } else {
            rdf
        };
        assert_eq!(
            rewrite_unedited(
                original.as_bytes(),
                &MetadataExportOptions::default(),
                &cancel
            )
            .expect("all Keep"),
            original.as_bytes()
        );
        for edited in [false, true] {
            let mut packet = original.as_bytes().to_vec();
            let mut first_size = None;
            for genre in ["one", "two", "one", "two"] {
                let mut options = MetadataExportOptions::default();
                options
                    .set(MetadataField::Genre, Some(genre.into()))
                    .expect("genre");
                options
                    .set(MetadataField::Artist, Some("new artist".into()))
                    .expect("artist");
                options
                    .set(MetadataField::Title, Some("new title".into()))
                    .expect("title");
                packet = if edited {
                    rewrite_edited(&packet, &options, &cancel)
                } else {
                    rewrite_unedited(&packet, &options, &cancel)
                }
                .expect("rewrite");
                let mut values = vec![];
                languages(
                    &tree(&packet, &cancel).expect("output XML"),
                    "",
                    &mut values,
                );
                for (text, language) in [
                    ("kept owner", expected_owner_language),
                    (genre, ""),
                    ("new artist", ""),
                    ("new title", "x-default"),
                ] {
                    assert!(
                        values.contains(&(text.into(), language.into())),
                        "language context changed for {text}: {values:?}"
                    );
                }
                assert_eq!(
                    packet.len(),
                    *first_size.get_or_insert(packet.len()),
                    "repeated updates must not add descriptions"
                );
            }
        }
    }
}

#[test]
fn xmp_declaration_is_unique_and_precedes_packet_wrappers() {
    let cancel = AtomicBool::new(false);
    let root = format!("<r:RDF xmlns:r=\"{RDF}\"><r:Description/></r:RDF>");
    let declaration = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
    for prefix in [
        String::new(),
        declaration.into(),
        format!("\u{feff}{declaration}"),
        format!("{declaration}\n<?xpacket begin=\"\"?>"),
        "\n<!-- packet --><?xpacket begin=\"\"?>".into(),
    ] {
        let packet = format!("{prefix}{root}<?xpacket end=\"w\"?>");
        assert!(
            parse(packet.as_bytes(), &cancel)
                .expect("valid prolog")
                .is_empty()
        );
        assert_eq!(
            rewrite_unedited(
                packet.as_bytes(),
                &MetadataExportOptions::default(),
                &cancel
            )
            .expect("unchanged packet"),
            packet.as_bytes()
        );
    }
    for prefix in [
        format!("{declaration}{declaration}"),
        format!(" {declaration}"),
        format!("\r\n{declaration}"),
        format!("<!-- packet -->{declaration}"),
        format!("<?xpacket begin=\"\"?>{declaration}"),
        format!("\u{feff}\u{feff}{declaration}"),
    ] {
        let packet = format!("{prefix}{root}");
        assert!(
            parse(packet.as_bytes(), &cancel).is_err(),
            "invalid prolog accepted: {prefix:?}"
        );
        for title in [None, Some("Changed title")] {
            let mut options = MetadataExportOptions::default();
            options
                .set(MetadataField::Title, title.map(str::to_owned))
                .expect("title");
            assert!(rewrite_unedited(packet.as_bytes(), &options, &cancel).is_err());
            assert!(rewrite_edited(packet.as_bytes(), &options, &cancel).is_err());
        }
    }
}

#[test]
fn edited_xmp_keeps_keywords_without_promoting_foreign_or_nested_properties() {
    let cancel = AtomicBool::new(false);
    for subject in [
        r#"<d:subject><r:Bag><r:li>nature &amp; travel</r:li><r:li><![CDATA[<landscape>]]></r:li><r:li>nature &amp; travel</r:li></r:Bag></d:subject>"#,
        r#"<d:subject><r:Bag/></d:subject>"#,
    ] {
        let packet = format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:d=\"{DC}\" xmlns:t=\"http://ns.adobe.com/tiff/1.0/\" xml:lang=\"en\" t:ImageWidth=\"1234\">{subject}<d:subject xmlns:d=\"urn:not-dc\">foreign keyword</d:subject><t:opaque><d:subject>nested keyword</d:subject></t:opaque></r:Description></r:RDF>"
        );
        for title in [None, Some("New title"), Some("")] {
            let mut options = MetadataExportOptions::default();
            options
                .set(MetadataField::Title, title.map(str::to_owned))
                .expect("title option");
            let output = rewrite_edited(packet.as_bytes(), &options, &cancel).expect("rewrite");
            let text = std::str::from_utf8(&output).expect("UTF-8");
            assert!(text.contains(subject), "keyword structure must survive");
            assert!(text.contains("xml:lang=\"en\""));
            for removed in ["t:ImageWidth=", "foreign keyword", "nested keyword"] {
                assert!(!text.contains(removed), "must not retain {removed}");
            }
            assert_eq!(
                rewrite_edited(&output, &options, &cancel).expect("resave"),
                output
            );
        }
    }
    // Preserve an existing attribute spelling too; keyword retention does not
    // interpret or normalize its RDF representation into editable fields.
    let packet = format!(
        "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:s=\"{DC}\" s:subject=\"one &amp; two\"/></r:RDF>"
    );
    assert_eq!(
        rewrite_edited(
            packet.as_bytes(),
            &MetadataExportOptions::default(),
            &cancel
        )
        .expect("attribute-only keywords"),
        packet.as_bytes()
    );
    assert!(
        parse(packet.as_bytes(), &cancel)
            .expect("editable fields")
            .is_empty()
    );
}

#[test]
fn edited_xmp_keeps_rights_scopes_but_drops_geometry_and_asset_identity() {
    let cancel = AtomicBool::new(false);
    let terms = r#"<q:UsageTerms><r:Alt><r:li xml:lang="en">Keep &amp; attribute</r:li><r:li xml:lang="fr"><![CDATA[Termes <originaux>]]></r:li></r:Alt></q:UsageTerms>"#;
    let packet = format!(
        "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:q=\"{RIGHTS}\" xmlns:d=\"{DC}\" xmlns:t=\"http://ns.adobe.com/tiff/1.0/\" xmlns:mm=\"http://ns.adobe.com/xap/1.0/mm/\" xml:lang=\"fr\" q:Marked=\"True\" t:Orientation=\"6\" mm:DocumentID=\"original-asset\"><d:rights><r:Alt><r:li xml:lang=\"x-default\">Notice</r:li></r:Alt></d:rights>{terms}<q:Owner><r:Bag><r:li>Owner A</r:li><r:li>Owner B</r:li></r:Bag></q:Owner><q:WebStatement>https://example.invalid/rights</q:WebStatement><q:Certificate>original-certificate</q:Certificate><q:Unknown>unknown-rights-field</q:Unknown><q:Marked xmlns:q=\"urn:not-rights\">foreign-property</q:Marked><t:ImageWidth>1234</t:ImageWidth></r:Description></r:RDF>"
    );
    for replacement in [None, Some("New notice"), Some("")] {
        let mut options = MetadataExportOptions::default();
        options
            .set(MetadataField::Copyright, replacement.map(str::to_owned))
            .expect("copyright");
        options
            .set(MetadataField::Title, Some("New title".into()))
            .expect("new title");
        let output = rewrite_edited(packet.as_bytes(), &options, &cancel).expect("edited metadata");
        let text = std::str::from_utf8(&output).expect("UTF-8");
        for preserved in [
            terms,
            "q:Marked=\"True\"",
            "Owner A",
            "Owner B",
            "https://example.invalid/rights",
            "xml:lang=\"fr\"",
        ] {
            assert!(text.contains(preserved), "missing {preserved}");
        }
        for stale in [
            "t:Orientation=",
            "original-asset",
            "original-certificate",
            "unknown-rights-field",
            "foreign-property",
            "<t:ImageWidth>",
        ] {
            assert!(!text.contains(stale), "stale {stale}");
        }
        let mut expected = parse(packet.as_bytes(), &cancel).expect("original");
        apply(&mut expected, &options).expect("requested");
        assert_eq!(parse(&output, &cancel).expect("readback"), expected);
        assert_eq!(
            rewrite_edited(&output, &options, &cancel).expect("resave"),
            output
        );
        assert!(matches!(
            rewrite_edited(&output, &options, &AtomicBool::new(true)),
            Err(ExportError::Cancelled)
        ));
    }
    let technical_only = format!(
        "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:t=\"http://ns.adobe.com/tiff/1.0/\" t:Orientation=\"6\"/></r:RDF>"
    );
    assert!(
        rewrite_edited(
            technical_only.as_bytes(),
            &MetadataExportOptions::default(),
            &cancel
        )
        .expect("no transferable properties")
        .is_empty()
    );
    assert!(
        rewrite_edited(
            b"<!DOCTYPE x><x/>",
            &MetadataExportOptions::default(),
            &cancel
        )
        .is_err()
    );
}

#[test]
fn unedited_xmp_rewrite_preserves_opaque_structure_and_namespace_scopes() {
    let opaque = r#"<p:opaque p:flag="a&amp;b">before<![CDATA[<raw>]]><p:part xmlns:d="urn:not-dc"><d:title>nested title</d:title></p:part>after<!--keep--></p:opaque>"#;
    let packet = format!(
        "\u{feff}<?xml version=\"1.0\"?><?xpacket begin=\"\"?><x:xmpmeta xmlns:x=\"{META}\" x:toolkit=\"kept\"><r:RDF xmlns:r=\"{RDF}\" xmlns:rdf=\"urn:shadow\"><r:Description xmlns:d=\"{DC}\" xmlns:m=\"{DM}\" xmlns:p=\"urn:opaque\" r:about=\"\" m:genre='old &amp; genre' p:flag='a&amp;b'><d:title><r:Alt><r:li xml:lang=\"x-default\">old title</r:li><r:li xml:lang=\"ja\">old translation</r:li></r:Alt></d:title>{opaque}</r:Description><r:Description xmlns:m=\"{DM}\" m:album=\"retained album\"/><!--tail--></r:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );
    let cancel = AtomicBool::new(false);
    assert_eq!(
        rewrite_unedited(
            packet.as_bytes(),
            &MetadataExportOptions::default(),
            &cancel
        )
        .expect("Keep"),
        packet.as_bytes()
    );
    for title in ["new <title>\r&", ""] {
        let mut options = MetadataExportOptions::default();
        options
            .set(MetadataField::Title, Some(title.into()))
            .expect("title");
        options
            .set(MetadataField::Genre, Some("new genre".into()))
            .expect("genre");
        options
            .set(MetadataField::Composer, Some("new composer".into()))
            .expect("new field");
        let output = rewrite_unedited(packet.as_bytes(), &options, &cancel).expect("rewrite");
        let text = std::str::from_utf8(&output).expect("UTF-8");
        assert!(text.starts_with('\u{feff}') && text.ends_with("<?xpacket end=\"w\"?>"));
        assert!(
            text.contains(opaque),
            "mixed opaque content is not reconstructed"
        );
        assert!(text.contains("x:toolkit=\"kept\"") && text.contains("p:flag=\"a&amp;b\""));
        assert!(
            text.contains("<r:Description xmlns:m=")
                && text.contains("m:album=\"retained album\"/><!--tail-->")
        );
        assert!(
            !text.contains("old title")
                && !text.contains("old translation")
                && !text.contains("old &amp; genre")
        );
        let mut expected = parse(packet.as_bytes(), &cancel).expect("original values");
        apply(&mut expected, &options).expect("requested values");
        assert_eq!(parse(&output, &cancel).expect("result values"), expected);
        assert_eq!(
            rewrite_unedited(&output, &MetadataExportOptions::default(), &cancel).expect("resave"),
            output
        );
    }
}

#[test]
fn repeated_xmp_updates_do_not_accumulate_description_wrappers() {
    let opaque = "<p:opaque xmlns:p=\"urn:opaque\">kept</p:opaque>";
    for original in [
        format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description>{opaque}</r:Description><r:Description/></r:RDF>"
        ),
        format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description/><!--kept--><r:Description>{opaque}</r:Description></r:RDF>"
        ),
        format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description xml:lang=\"fr\">{opaque}</r:Description></r:RDF>"
        ),
    ] {
        let cancel = AtomicBool::new(false);
        let mut packet = original.into_bytes();
        let mut first_size = None;
        for index in 0..64 {
            let mut options = MetadataExportOptions::default();
            options
                .set(
                    MetadataField::Title,
                    Some(if index % 2 == 0 { "one" } else { "two" }.into()),
                )
                .expect("title");
            packet = rewrite_unedited(&packet, &options, &cancel).expect("repeated update");
            assert_eq!(
                packet.len(),
                *first_size.get_or_insert(packet.len()),
                "same-length replacement {index} grows the packet"
            );
            assert!(
                std::str::from_utf8(&packet)
                    .expect("UTF-8")
                    .contains(opaque)
            );
            assert_eq!(
                rewrite_unedited(&packet, &options, &cancel).expect("same setting"),
                packet,
                "reapplying a setting should not change the packet"
            );
        }
    }
}

#[test]
fn unedited_xmp_rewrite_handles_empty_roots_removal_limits_and_cancellation() {
    let cancel = AtomicBool::new(false);
    for (field, local, array) in [
        (MetadataField::Title, "title", "Alt"),
        (MetadataField::Artist, "creator", "Seq"),
    ] {
        let packet = format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:dc=\"{DC}\"><dc:{local}><r:{array}/></dc:{local}></r:Description></r:RDF>"
        );
        assert!(
            parse(packet.as_bytes(), &cancel)
                .expect("empty list")
                .is_empty()
        );
        let mut remove = MetadataExportOptions::default();
        remove.set(field, Some(String::new())).expect("remove");
        let output =
            rewrite_unedited(packet.as_bytes(), &remove, &cancel).expect("remove empty list");
        assert!(
            !std::str::from_utf8(&output)
                .expect("UTF-8")
                .contains(&format!("dc:{local}"))
        );
        assert_eq!(
            rewrite_unedited(&output, &remove, &cancel).expect("repeated Remove"),
            output
        );
        let empty_root = format!("<r:RDF xmlns:r=\"{RDF}\"/>");
        assert_eq!(
            rewrite_unedited(empty_root.as_bytes(), &remove, &cancel).expect("absent property"),
            empty_root.as_bytes()
        );
    }
    let mut options = MetadataExportOptions::default();
    options
        .set(MetadataField::Title, Some("added".into()))
        .expect("title");
    for packet in [
        format!("<RDF xmlns=\"{RDF}\"/>"),
        format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:d=\"{DC}\" d:title=\"attribute title\"/></r:RDF>"
        ),
        format!(
            "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:d=\"{DC}\"><d:title/></r:Description></r:RDF>"
        ),
    ] {
        let output =
            rewrite_unedited(packet.as_bytes(), &options, &cancel).expect("insert/replace");
        assert_eq!(parse(&output, &cancel).expect("values")[0].text, "added");
        let mut remove = MetadataExportOptions::default();
        remove
            .set(MetadataField::Title, Some(String::new()))
            .expect("remove");
        let removed = rewrite_unedited(&output, &remove, &cancel).expect("remove property");
        assert!(parse(&removed, &cancel).expect("empty values").is_empty());
        assert!(
            !std::str::from_utf8(&removed)
                .expect("UTF-8")
                .contains("dc:title")
        );
    }
    let packet = format!(
        "<r:RDF xmlns:r=\"{RDF}\"><r:Description xmlns:p=\"urn:opaque\"><p:data>{}</p:data></r:Description></r:RDF>",
        "x".repeat(LIMIT - 180)
    );
    assert!(packet.len() < LIMIT);
    assert!(parse(packet.as_bytes(), &cancel).is_ok());
    assert!(
        rewrite_unedited(packet.as_bytes(), &options, &cancel)
            .expect_err("output limit")
            .to_string()
            .contains("exceeds")
    );
    assert!(matches!(
        rewrite_unedited(packet.as_bytes(), &options, &AtomicBool::new(true)),
        Err(ExportError::Cancelled)
    ));
    assert!(rewrite_unedited(b"<!DOCTYPE x><x/>", &options, &cancel).is_err());
}
