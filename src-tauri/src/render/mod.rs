//! Mail rendering pipeline (spec §8 v2, D-014): ammonia sanitization →
//! lol_html image neutralization (with per-sender/per-message "show images"
//! overrides routed through the app's own fetch proxy) → link cleaning →
//! sandboxed-iframe document.
//!
//! v2 supersedes the v1 single-ammonia-pass (D-026): ammonia still strips
//! scripts/iframes/handlers and cleans link parameters, but remote images now
//! flow through the lol_html pass which can either BLOCK them (placeholder +
//! count, the default) or REWRITE them to the loopback image proxy
//! (`Show images` / per-sender allowlist). The output carries its own CSP
//! meta, so the reader's sandboxed iframe (`sandbox=""`) stays
//! defense-in-depth.

use std::borrow::Cow;
use std::sync::Arc;

use ammonia::Builder;
use lol_html::{element, rewrite_str, RewriteStrSettings};
use url::Url;

/// Tracking parameters stripped from link hrefs (spec §8.3: `utm_*` plus the
/// universal click-ids; the bundled redirect-prefix list is Phase 4).
const TRACKING_PARAMS: [&str; 4] = ["fbclid", "gclid", "mc_eid", "msclkid"];

/// Known email-tracker domains (hand-curated from permissively-licensed
/// sources + widely documented vendors, D-014). Substring match on the image
/// host. Updates are opt-in only (D-015/D-016); this list never auto-grows.
const TRACKER_DOMAINS: [&str; 10] = [
    "list-manage",
    "campaign-archive",
    "mandrill",
    "sendgrid",
    "hubspot",
    "hsforms",
    "marketo",
    "clickdimensions",
    "exacttarget",
    "salesforce",
];

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

/// How remote images are treated for this render (FR-15/FR-19 overrides).
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Per-message "Show images" or per-sender allowlist. `false` (default):
    /// images are blocked and counted.
    pub allow_remote_images: bool,
    /// Origin of the loopback image proxy (`http://127.0.0.1:port`), present
    /// when the proxy is running. Allowed images are rewritten to proxy URLs
    /// so the fetch carries no referer/cookies and is visible in Diagnostics.
    /// Without a proxy, allowed images load directly from the webview (the
    /// CSP gains `https: img-src`) — the honest-IP caveat applies either way.
    pub proxy_origin: Option<String>,
}

/// Sanitized document ready for the reader iframe's `srcdoc`.
pub struct Rendered {
    /// Complete HTML document (CSP meta included).
    pub html: String,
    /// Remote images blocked by policy (feeds the FR-15 badge).
    pub blocked_images: usize,
    /// Images that matched tracker heuristics (pixel-shaped or known tracker
    /// domain) — the "Blocked N trackers" badge counts these (FR-15).
    pub tracking_count: usize,
}

fn csp_for(allow: bool, proxy_origin: Option<&str>) -> String {
    let img_src = match (allow, proxy_origin) {
        (true, Some(origin)) => format!("data: {origin}"),
        (true, None) => "data: https:".to_string(),
        (false, _) => "data:".to_string(),
    };
    format!(
        r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {img_src}; style-src 'unsafe-inline'">"#
    )
}

/// The v2 pipeline over one message's raw HTML.
pub fn render_document_opts(raw_html: &str, opts: &RenderOptions) -> Rendered {
    let blocked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let trackers = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Step 1: ammonia whitelist sanitization (strip script/iframe/form/object,
    // on* handlers, javascript:/data: URLs). Images pass through untouched so
    // lol_html can decide their fate.
    let mut builder = Builder::new();
    builder.add_clean_content_tags(["iframe", "form", "object", "embed"]);
    builder.url_schemes(
        std::iter::once("http")
            .chain(std::iter::once("https"))
            .collect(),
    );
    builder.add_tag_attributes("img", ["src", "srcset", "width", "height"]);
    builder.attribute_filter(|_element, attribute, value| match attribute {
        "href" => clean_href(value),
        _ => Some(Cow::Borrowed(value)),
    });
    let sanitized = builder.clean(raw_html).to_string();

    // Step 2: lol_html neutralization of remote images.
    let blocked_c = Arc::clone(&blocked);
    let trackers_c = Arc::clone(&trackers);
    let allow = opts.allow_remote_images;
    let proxy_origin = opts.proxy_origin.clone();
    let handler = element!("img", move |el: &mut lol_html::html_content::Element| {
        el.remove_attribute("srcset");
        let Some(src) = el.get_attribute("src") else {
            return Ok(());
        };
        // cid: references were already dropped by ammonia's scheme filter.
        let Ok(parsed) = Url::parse(&src) else {
            el.remove_attribute("src");
            return Ok(());
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            el.remove_attribute("src");
            return Ok(());
        }
        let is_tracker = pixel_shaped(el) || {
            parsed
                .host_str()
                .map(|h| {
                    let h = h.to_lowercase();
                    TRACKER_DOMAINS.iter().any(|d| h.contains(d))
                })
                .unwrap_or(false)
        };
        if is_tracker {
            trackers_c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if is_tracker {
            // Tracker pixels stay blocked regardless of the allowlist —
            // that is the product's job (FR-15 badge stays honest).
            el.remove_attribute("src");
            el.set_attribute("data-blocked-src", &src).ok();
            el.set_attribute("data-blocked-img", "").ok();
            blocked_c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else if allow {
            // Route through the app's fetch proxy (no referer, no cookies);
            // fall back to a direct remote URL when no proxy is running.
            if let Some(origin) = &proxy_origin {
                let proxied = crate::render::proxy::proxied_url(origin, &src);
                el.set_attribute("src", &proxied).ok();
            }
        } else {
            // data-blocked-src keeps the URL inspectable without loading
            // it (spec §8.2 placeholder; its consumer is "Show images").
            el.remove_attribute("src");
            el.set_attribute("data-blocked-src", &src).ok();
            el.set_attribute("data-blocked-img", "").ok();
            blocked_c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    });
    let rewritten = rewrite_str(
        &sanitized,
        RewriteStrSettings::new().append_element_content_handler(handler),
    )
    .unwrap_or(sanitized);

    // Steps 3 (link cleaning) ran inside ammonia's attribute filter.
    // Step 4: the sandboxed-iframe document with a matching CSP meta.
    let csp = csp_for(opts.allow_remote_images, opts.proxy_origin.as_deref());
    Rendered {
        html: format!(
            "<!DOCTYPE html><html><head>{csp}<base target=\"_blank\"><style>body{{margin:0;font:14px system-ui;padding:12px}}img[data-blocked-img]{{display:none}}</style></head><body>{rewritten}</body></html>"
        ),
        blocked_images: blocked.load(std::sync::atomic::Ordering::Relaxed),
        tracking_count: trackers.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Pixel heuristics (spec §8.2): explicit 1×1 dimensions.
fn pixel_shaped(el: &lol_html::html_content::Element) -> bool {
    let dim = |name: &str| {
        el.get_attribute(name)
            .and_then(|v| v.trim().parse::<u32>().ok())
    };
    matches!((dim("width"), dim("height")), (Some(1), Some(1)))
        || (dim("width").is_none() && dim("height").is_none() && el.has_attribute("data-pixel"))
}

/// v1-compatible entry point (default policy: block all remote images).
pub fn render_document(raw_html: &str) -> Rendered {
    render_document_opts(raw_html, &RenderOptions::default())
}

pub mod proxy;

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
        assert!(
            !h.contains(" src="),
            "no live remote src (URL survives only in data-blocked-src): {h}"
        );
        assert!(h.contains("data-blocked-src"), "placeholder present: {h}");
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
    fn document_shape_carries_csp_meta_and_link_guard() {
        let out = render_document("<p>hello</p>");
        assert!(out.html.starts_with("<!DOCTYPE html>"));
        assert!(out.html.contains("Content-Security-Policy"));
        assert!(out.html.contains("default-src 'none'"));
        // Links must target _blank so the sandbox (no allow-popups) makes
        // them inert instead of navigating the reader frame off-site.
        assert!(out.html.contains(r#"<base target="_blank">"#));
        assert!(out.html.contains("hello"));
        assert_eq!(out.blocked_images, 0);
    }

    /// V-4.4: the tracked fixture — pixel-shaped image + known tracker domain
    /// both count as trackers; the blocked URL survives only in the
    /// data-blocked-src placeholder.
    #[test]
    fn trackers_are_counted_and_blocked() {
        let out = render_document(
            r#"<img src="https://mailfoogane.a.sendgrid.net/open.gif" width="1" height="1">
               <img src="https://list-manage.com/track.png">"#,
        );
        assert_eq!(out.blocked_images, 2);
        assert_eq!(out.tracking_count, 2);
        let h = &out.html;
        assert!(h.contains("data-blocked-src"), "placeholder present: {h}");
        assert!(!h.contains(" src="), "no live remote src: {h}");
    }

    /// V-4.4: the allowlist override — allowed images are rewritten to the
    /// proxy origin and the CSP gains the proxy as an img-src.
    #[test]
    fn allowed_images_route_through_the_proxy() {
        let out = render_document_opts(
            r#"<img src="https://media.example/photo.jpg" srcset="https://media.example/photo@2x.jpg 2x">
               <img src="https://t.example/px.gif" width="1" height="1">"#,
            &RenderOptions {
                allow_remote_images: true,
                proxy_origin: Some("http://127.0.0.1:4567".into()),
            },
        );
        let h = &out.html;
        assert!(
            h.contains("http://127.0.0.1:4567/img?t="),
            "proxy rewrite: {h}"
        );
        assert!(!h.contains("srcset"), "srcset always dropped: {h}");
        // The tracker pixel stays blocked even when images are allowed.
        assert_eq!(out.blocked_images, 1);
        assert_eq!(out.tracking_count, 1);
        assert!(h.contains("img-src data: http://127.0.0.1:4567"), "{h}");
    }

    /// Allowed without a proxy: direct https load, CSP gains https:.
    #[test]
    fn allowed_images_without_proxy_load_directly() {
        let out = render_document_opts(
            r#"<img src="https://media.example/photo.jpg">"#,
            &RenderOptions {
                allow_remote_images: true,
                proxy_origin: None,
            },
        );
        assert!(out.html.contains("src=\"https://media.example/photo.jpg\""));
        assert!(
            out.html.contains("img-src data: https:"),
            "{h}",
            h = out.html
        );
        assert_eq!(out.blocked_images, 0);
    }

    #[test]
    fn cid_references_stay_dropped() {
        let out = render_document(r#"<img src="cid:logo">"#);
        assert!(!out.html.contains("cid:"));
        assert_eq!(out.blocked_images, 0, "cid is not a remote image");
    }
}
