//! Regression cases derived from ammonia 4.1.8's public Builder contract and
//! upstream tests `url_cid_in_css`, `url_scheme_filter_in_css`, and
//! `ns_svg_animatetransformmotioncolor_url_attr`.
//! Advisory references: RUSTSEC-2021-0157, RUSTSEC-2021-0158, and
//! RUSTSEC-2026-0373. The two 2021 identifiers are the published RustSec IDs.

use ammonia::Builder;

#[test]
fn mixed_case_quoted_css_urls_obey_the_allowed_scheme_policy() {
    let mut sanitizer = Builder::default();
    sanitizer
        .add_generic_attributes(["style"])
        .filter_style_properties(["background-image"].into())
        .url_schemes(["cid"].into());
    for function in ["url", "URL", "uRl", "UrL"] {
        let html = format!(
            "<p style='background-image:{function}(\"https://tracker.invalid/pixel\")'>Family note</p>"
        );
        let clean = sanitizer.clean(&html).to_string();
        assert_eq!(clean, "<p style=\"\">Family note</p>", "{function}");
    }
    // Prevent an always-strip-style implementation from satisfying the test.
    let allowed = sanitizer
        .clean(r#"<p style='background-image:URL("cid:family-portrait")'>Family note</p>"#)
        .to_string();
    assert!(allowed.contains("cid:family-portrait"));
    assert!(allowed.contains("background-image:url("));
    assert!(allowed.contains("Family note"));
}

#[test]
fn quoted_css_url_cannot_break_out_into_a_new_declaration() {
    let mut sanitizer = Builder::default();
    sanitizer
        .add_generic_attributes(["style"])
        .filter_style_properties(["background"].into())
        .url_schemes(["cid"].into());
    // The apparent declaration is part of one quoted CID URL. It must remain
    // escaped when serialized, rather than becoming a new CSS declaration.
    // Concrete vector: https://github.com/rust-ammonia/ammonia/pull/259
    let clean = sanitizer
        .clean(r#"<div style='background:url("cid:\");position:fixed;background:url(\"cid:")'>Family note</div>"#)
        .to_string();
    assert_eq!(
        clean,
        r#"<div style="background:url(&quot;cid:\&quot;);position:fixed;background:url(\&quot;cid:&quot;)">Family note</div>"#
    );
    assert_eq!(sanitizer.clean(&clean).to_string(), clean);
    let allowed = sanitizer
        .clean(r#"<div style='background:url("cid:family-portrait")'>Family note</div>"#)
        .to_string();
    assert!(allowed.contains("background:url("));
    assert!(allowed.contains("cid:family-portrait"));
}

#[test]
fn alternate_svg_animation_elements_cannot_introduce_javascript_links() {
    let mut sanitizer = Builder::default();
    let tags = ["animateMotion", "animateTransform", "animateColor"];
    sanitizer.add_tags(["svg", "a"]).add_tags(tags);
    for tag in tags {
        sanitizer.add_tag_attributes(tag, ["attributeName", "values", "from", "to"]);
    }
    // These forms were missed by the earlier animate/set fix. Explicitly
    // allowing them is necessary to exercise RUSTSEC-2021-0158.
    for tag in tags {
        for attributes in [
            r#"to="javascript:alert(1)""#,
            r#"from="javascript:alert(1)" to="https://example.org/""#,
            r#"values="https://example.org/;javascript:alert(1)""#,
        ] {
            let clean = sanitizer
                .clean(&format!(
                    "<svg><a><{tag} attributeName=\"href\" {attributes}></{tag}></a></svg>"
                ))
                .to_string();
            assert!(
                !clean.to_ascii_lowercase().contains("javascript:"),
                "{tag} preserved an executable URL: {clean}"
            );
            assert!(!clean.contains("alert(1)"));
            assert!(clean.contains("<svg>"), "the allowed SVG is preserved");
        }
        // A sanitizer that drops every animation cannot satisfy this control.
        let allowed = sanitizer
            .clean(&format!(
                "<svg><a><{tag} attributeName=\"href\" to=\"https://example.org/\"></{tag}></a></svg>"
            ))
            .to_string();
        assert!(allowed.contains(&format!("<{tag}")));
        assert!(allowed.contains("to=\"https://example.org/\""));
    }
}

#[test]
fn ordinary_family_html_keeps_formatting_and_safe_links() {
    let clean = ammonia::clean(
        r#"<p>Meet <strong>Nova</strong>, our <em>curious</em> companion. <a href="https://example.org/family">Read our story</a></p>"#,
    );
    assert!(clean.contains("<strong>Nova</strong>"));
    assert!(clean.contains("<em>curious</em>"));
    assert!(clean.contains("href=\"https://example.org/family\""));
    assert!(clean.contains("Read our story</a>"));
    assert!(!clean.contains("&lt;p&gt;"));
}
