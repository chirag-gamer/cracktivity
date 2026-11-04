//! Mail rendering pipeline (spec §8 v1, D-014): one ammonia pass that
//! sanitizes attacker-controlled HTML, blocks **all** remote images (v1 has
//! no tracker list and no "show images" override — those land with lol_html
//! in Phase 4), and strips known tracking parameters from links.
//!
//! The output is a full srcdoc document carrying its own CSP meta tag, so
//! the reader's sandboxed iframe (spec §8.4: `sandbox=""`, no scripts) is
//! defense-in-depth on top of this pass — the iframe blocks script even if
//! sanitization ever regresses.

use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ammonia::Builder;
use url::Url;

/// Tracking parameters stripped from link hrefs (spec §8.3: `utm_*` plus
/// the universal click-ids; the bundled redirect-prefix list is Phase 4).
const TRACKING_PARAMS: [&str; 4] = ["fbclid", "gclid", "mc_eid", "msclkid"];

fn is_tracking_param(key: &str) -> bool {
    key.starts_with("utm_") || TRACKING_PARAMS.contains(&key)
}

fn clean_href(value: &str) -> Option<Cow<'_, str>> {
    // Only absolute http(s) URLs carry tracking params worth stripping;
    // everything else passes through untouched (ammonia's scheme filter has
    // already dropped javascript:/data: before we see hrefs it keeps).
    let Ok(mut url) = Url::parse(value) else {
        return Some(Cow::Borrowed(value));
    };
    let mut dirty = false;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| {
            let tracking = is_tracking_param(k);
            dirty |= tracking;
            !tracking
        })
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if !dirty {
        return Some(Cow::Borrowed(value));
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    Some(Cow::Owned(url.to_string()))
}

/// Sanitized document ready for the reader iframe's `srcdoc`.
pub struct Rendered {
    /// Complete HTML document (CSP meta included).
    pub html: String,
    /// Remote images blocked (feeds the FR-15 badge from Phase 4 on).
    pub blocked_images: usize,
}

const CSP_META: &str =
    r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:">"#;

/// The v1 pipeline over one message's raw HTML.
pub fn render_document(raw_html: &str) -> Rendered {
    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&blocked);

    let mut builder = Builder::new();
    // Defaults already exclude script/style/iframe/form/object/embed/meta/
    // link/base tags and all on* attributes; also drop those elements'
    // content wholesale so no payload survives in discarded wrappers.
    builder.add_clean_content_tags(["iframe", "form", "object", "embed"]);
    // Only http(s) survives on href/src: javascript:, data:, cid: are gone.
    builder.url_schemes(
        std::iter::once("http")
            .chain(std::iter::once("https"))
            .collect(),
    );
    // srcset is not in ammonia's default img allowlist; adding it lets the
    // filter see (and count) every remote image reference before stripping.
    builder.add_tag_attributes("img", ["srcset"]);
    builder.attribute_filter(move |element, attribute, value| {
        match (element, attribute) {
            // Block ALL remote images (v1 policy): strip the attribute and
            // count it. cid: images never reach here (scheme filter).
            ("img", "src") | ("img", "srcset") => {
                counter.fetch_add(1, Ordering::Relaxed);
                None
            }
            ("a", "href") => clean_href(value),
            _ => Some(Cow::Borrowed(value)),
        }
    });

    let body = builder.clean(raw_html).to_string();
    Rendered {
        html: format!(
            "<!DOCTYPE html><html><head>{CSP_META}<style>body{{margin:0;font:14px system-ui;padding:12px}}img{{display:none}}</style></head><body>{body}</body></html>"
        ),
        blocked_images: blocked.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_iframes_and_handlers_are_stripped() {
        let out = render_document(
            r#"<div><script>alert(1)</script><p onclick="x()">hi</p>
                <iframe src="https://evil.example"></iframe>
                <img src="https://t.example/px.gif"></div>"#,
        );
        let h = out.html;
        assert!(!h.contains("script"), "no script tag: {h}");
        assert!(!h.contains("alert"), "script content cleaned: {h}");
        assert!(!h.contains("iframe"), "iframe removed: {h}");
        assert!(!h.contains("onclick"), "handlers stripped: {h}");
        assert_eq!(out.blocked_images, 1);
        assert!(!h.contains("px.gif"), "remote img blocked: {h}");
    }

    #[test]
    fn links_keep_href_with_rel_and_lose_tracking_params() {
        let out = render_document(
            r#"<a href="https://example.com/p?utm_source=news&amp;fbclid=abc&keep=1">x</a>
               <a href="javascript:evil()">y</a>
               <a href="https://plain.example/page">z</a>"#,
        );
        let h = &out.html;
        assert!(h.contains("keep=1"), "harmless param survives: {h}");
        assert!(!h.contains("utm_source"), "utm stripped: {h}");
        assert!(!h.contains("fbclid"), "fbclid stripped: {h}");
        assert!(h.contains("rel=\"noopener noreferrer\""), "rel forced: {h}");
        assert!(!h.contains("javascript:"), "js href dropped: {h}");
        assert!(
            h.contains("https://plain.example/page"),
            "clean link intact: {h}"
        );
    }

    #[test]
    fn document_shape_carries_csp_meta() {
        let out = render_document("<p>hello</p>");
        assert!(out.html.starts_with("<!DOCTYPE html>"));
        assert!(out.html.contains("Content-Security-Policy"));
        assert!(out.html.contains("default-src 'none'"));
        assert!(out.html.contains("hello"));
        assert_eq!(out.blocked_images, 0);
    }

    #[test]
    fn srcset_counts_and_cid_is_dropped() {
        let out = render_document(
            r#"<img srcset="https://a.example/1.png 1x, https://a.example/2.png 2x" src="cid:logo">"#,
        );
        // srcset is one blocked remote reference; the cid: src is dropped by
        // the scheme allowlist before the filter runs (blocked either way,
        // just not double-counted).
        assert_eq!(out.blocked_images, 1);
        assert!(!out.html.contains("a.example"));
        assert!(!out.html.contains("cid:"));
    }
}
