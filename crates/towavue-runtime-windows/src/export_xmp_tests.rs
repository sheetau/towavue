use super::*;

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
