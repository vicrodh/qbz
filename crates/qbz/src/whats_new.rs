//! What's New modal controller + release-notes markdown renderer.
//!
//! On open, fetches the GitHub release whose tag matches the running version
//! (`https://api.github.com/repos/vicrodh/qbz/releases/tags/v{version}`) on a
//! worker thread, parses its markdown `body` into a flat block model, and hops
//! back to the Slint event loop to fill `WhatsNewState`.
//!
//! The markdown renderer is a 1:1 port of the Tauri `renderMarkdownWithToc`
//! (`src/lib/utils/markdown.ts`): it supports the same small subset — `#`/`##`
//! headings AND indent-0 `- ` bullets both become level-0 SECTIONS (the TOC),
//! `###` becomes a sub-section, indented bullets nest by `floor(spaces/2)`, and
//! everything else is a paragraph. Inline `**bold**` / `` `code` `` markers are
//! STRIPPED (Slint has no inline rich-text spans in a single Text — accepted
//! deviation; the plain text is preserved).

use serde::Deserialize;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::{AppWindow, WhatsNewActions, WhatsNewBlock, WhatsNewState, WhatsNewTocEntry};

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/vicrodh/qbz/releases";

/// Block kinds shared with `WhatsNewBlock.kind` in the Slint model.
const KIND_SECTION: i32 = 0;
const KIND_BULLET: i32 = 1;
const KIND_PARAGRAPH: i32 = 2;
/// A whole-line markdown link `[text](url)` — rendered as a clickable link.
const KIND_LINK: i32 = 3;

/// GitHub release JSON (only the fields the modal needs).
#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    published_at: String,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The parsed release the controller applies to the UI.
struct FetchedRelease {
    version: String,
    date: String,
    body: Option<String>,
}

/// Wire the `WhatsNewActions` callbacks. Call once at shell setup. `handle` runs
/// the network fetch off the UI thread.
pub fn install(window: &AppWindow, handle: tokio::runtime::Handle) {
    // close() — just hide.
    {
        let weak = window.as_weak();
        window.global::<WhatsNewActions>().on_close(move || {
            if let Some(w) = weak.upgrade() {
                w.global::<WhatsNewState>().set_open(false);
            }
        });
    }

    // open_url() — open a standalone release-notes link in the browser.
    window.global::<WhatsNewActions>().on_open_url(|url| {
        let url = url.to_string();
        if let Err(e) = open::that(&url) {
            log::warn!("[qbz-slint] whats-new open-url failed for {url}: {e}");
        }
    });

    // open() — show, mark loading, then fetch + render on a worker thread.
    {
        let weak = window.as_weak();
        let handle = handle.clone();
        window.global::<WhatsNewActions>().on_open(move || {
            let version = crate::about::app_version().to_string();
            // Paint the modal immediately in its loading state.
            if let Some(w) = weak.upgrade() {
                let st = w.global::<WhatsNewState>();
                st.set_open(true);
                st.set_loading(true);
                st.set_has_body(false);
                st.set_version(version.clone().into());
                st.set_date("".into());
                st.set_blocks(ModelRc::new(VecModel::from(Vec::<WhatsNewBlock>::new())));
                st.set_toc(ModelRc::new(VecModel::from(Vec::<WhatsNewTocEntry>::new())));
            }

            let weak = weak.clone();
            handle.spawn(async move {
                let fetched = fetch_release_for_version(&version).await;
                let _ = weak.upgrade_in_event_loop(move |w| apply(&w, fetched));
            });
        });
    }
}

/// Apply the fetched release (or its absence) to `WhatsNewState`. Runs on the UI
/// thread.
fn apply(window: &AppWindow, fetched: Option<FetchedRelease>) {
    let st = window.global::<WhatsNewState>();
    st.set_loading(false);

    let Some(rel) = fetched else {
        st.set_has_body(false);
        return;
    };

    st.set_version(rel.version.into());
    st.set_date(rel.date.into());

    let body = rel.body.unwrap_or_default();
    let (blocks, toc) = render_markdown(&body);
    if blocks.is_empty() {
        st.set_has_body(false);
        return;
    }

    st.set_blocks(ModelRc::new(VecModel::from(blocks)));
    st.set_toc(ModelRc::new(VecModel::from(toc)));
    st.set_has_body(true);
}

/// Fetch the release for `version` by exact tag (`v{version}`), with the
/// GitHub-required `User-Agent`. Returns `None` on any network/parse failure or
/// for draft/prerelease tags (silent — the modal shows its empty state).
async fn fetch_release_for_version(version: &str) -> Option<FetchedRelease> {
    let tag = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };
    let url = format!("{GITHUB_RELEASES_URL}/tags/{tag}");

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("qbz")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[qbz-slint] whats-new client build failed: {e}");
            return None;
        }
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[qbz-slint] whats-new fetch failed for {tag}: {e}");
            return None;
        }
    };
    if !resp.status().is_success() {
        log::warn!("[qbz-slint] whats-new fetch HTTP {} for {tag}", resp.status());
        return None;
    }

    let release: GithubRelease = match resp.json().await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[qbz-slint] whats-new JSON parse failed for {tag}: {e}");
            return None;
        }
    };
    if release.draft || release.prerelease {
        return None;
    }

    Some(FetchedRelease {
        version: normalize_version_tag(&release.tag_name),
        date: format_release_date(&release.published_at),
        body: release.body,
    })
}

fn normalize_version_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

/// Format an RFC3339 timestamp as "Mon D, YYYY" (en-US short), mirroring the
/// Tauri `formatReleaseDate`. Falls back to the raw string on parse failure.
fn format_release_date(iso: &str) -> String {
    use chrono::{DateTime, Datelike};
    let Ok(dt) = DateTime::parse_from_rfc3339(iso) else {
        return iso.to_string();
    };
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m = dt.month0() as usize;
    let month = MONTHS.get(m).copied().unwrap_or("");
    format!("{} {}, {}", month, dt.day(), dt.year())
}

// ==================== Markdown → blocks + TOC ====================

/// Strip inline `**bold**` / `` `code` `` markers and reduce inline markdown
/// links `[text](url)` to just their `text` (a single Slint Text block can't
/// carry clickable inline spans — a WHOLE-line link becomes a `KIND_LINK` block
/// instead, see `parse_standalone_link`). Keeps the inner text otherwise.
fn strip_inline(text: &str) -> String {
    strip_markdown_links(text).replace("**", "").replace('`', "")
}

/// If `s[start..]` begins with a markdown link `[label](url)`, return
/// `(label, url, byte-index just past the ')')`. The `[](` `)` delimiters are
/// ASCII, so all returned slices sit on char boundaries. No nested brackets.
fn parse_link_at(s: &str, start: usize) -> Option<(&str, &str, usize)> {
    let rest = &s[start..];
    if !rest.starts_with('[') {
        return None;
    }
    let close_br = rest.find(']')?;
    if rest.as_bytes().get(close_br + 1) != Some(&b'(') {
        return None;
    }
    let open_paren = close_br + 1;
    let close_paren = open_paren + rest[open_paren..].find(')')?;
    let label = &rest[1..close_br];
    let url = &rest[open_paren + 1..close_paren];
    if url.is_empty() {
        return None;
    }
    Some((label, url, start + close_paren + 1))
}

/// Replace every inline `[text](url)` in a string with just its `text`.
fn strip_markdown_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text.as_bytes()[i] == b'[' {
            if let Some((label, _url, end)) = parse_link_at(text, i) {
                out.push_str(label);
                i = end;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// If the whole trimmed line is exactly one markdown link, return `(text, url)`.
fn parse_standalone_link(s: &str) -> Option<(&str, &str)> {
    let t = s.trim();
    let (label, url, end) = parse_link_at(t, 0)?;
    (end == t.len()).then_some((label, url))
}

/// Slugify a heading label into a TOC anchor id (port of the Tauri `slugify`).
fn slugify(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    // Drop markdown emphasis chars, then keep [a-z0-9] + spaces/hyphens.
    let mut cleaned = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if matches!(ch, '`' | '*' | '_' | '~') {
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == ' ' || ch == '-' {
            cleaned.push(ch);
        }
    }
    // Collapse whitespace runs to single hyphens, then collapse hyphen runs.
    let mut out = String::with_capacity(cleaned.len());
    let mut last_hyphen = false;
    for ch in cleaned.chars() {
        if ch == ' ' || ch == '-' {
            if !last_hyphen {
                out.push('-');
                last_hyphen = true;
            }
        } else {
            out.push(ch);
            last_hyphen = false;
        }
    }
    out.trim_matches('-').to_string()
}

/// Count leading-space indentation (a tab counts as 2), like the Tauri
/// `countLeadingSpaces`.
fn count_leading_spaces(line: &str) -> usize {
    let mut count = 0;
    for ch in line.chars() {
        match ch {
            ' ' => count += 1,
            '\t' => count += 2,
            _ => break,
        }
    }
    count
}

/// Push a section heading block; level-0 sections also become TOC entries.
fn push_heading(
    label: &str,
    level: i32,
    blocks: &mut Vec<WhatsNewBlock>,
    toc: &mut Vec<WhatsNewTocEntry>,
) {
    let clean = label.trim();
    if clean.is_empty() {
        return;
    }
    let id = slugify(clean);
    if level == 0 {
        toc.push(WhatsNewTocEntry {
            id: id.clone().into(),
            label: clean.into(),
        });
    }
    blocks.push(WhatsNewBlock {
        kind: KIND_SECTION,
        level,
        text: strip_inline(clean).into(),
        id: id.into(),
        url: "".into(),
    });
}

/// A clickable whole-line link block.
fn link_block(label: &str, url: &str) -> WhatsNewBlock {
    WhatsNewBlock {
        kind: KIND_LINK,
        level: 0,
        text: strip_inline(label).into(),
        id: "".into(),
        url: url.into(),
    }
}

/// Render the release-notes markdown into a flat block model + a TOC of the
/// level-0 section headings. 1:1 with `renderMarkdownWithToc`.
pub fn render_markdown(markdown: &str) -> (Vec<WhatsNewBlock>, Vec<WhatsNewTocEntry>) {
    let mut blocks: Vec<WhatsNewBlock> = Vec::new();
    let mut toc: Vec<WhatsNewTocEntry> = Vec::new();

    if markdown.trim().is_empty() {
        return (blocks, toc);
    }

    for line in markdown.split('\n') {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Headings (#, ##, ###).
        if let Some(rest) = trimmed.strip_prefix("# ") {
            push_heading(rest, 0, &mut blocks, &mut toc);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            push_heading(rest, 0, &mut blocks, &mut toc);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            push_heading(rest, 1, &mut blocks, &mut toc);
            continue;
        }

        // List items with indentation-based nesting.
        let is_list = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        if is_list {
            let indent = count_leading_spaces(line);
            let level = (indent / 2) as i32;
            let content = trimmed[2..].trim();

            if level == 0 {
                // Top-level bullets become section headings (no bullet glyph).
                push_heading(content, 0, &mut blocks, &mut toc);
                continue;
            }

            // A bullet that is nothing but a link renders as a clickable link.
            if let Some((label, url)) = parse_standalone_link(content) {
                blocks.push(link_block(label, url));
                continue;
            }

            blocks.push(WhatsNewBlock {
                kind: KIND_BULLET,
                level,
                text: strip_inline(content).into(),
                id: "".into(),
                url: "".into(),
            });
            continue;
        }

        // A paragraph that is nothing but a link renders as a clickable link.
        if let Some((label, url)) = parse_standalone_link(trimmed) {
            blocks.push(link_block(label, url));
            continue;
        }

        // Paragraph.
        blocks.push(WhatsNewBlock {
            kind: KIND_PARAGRAPH,
            level: 0,
            text: strip_inline(trimmed).into(),
            id: "".into(),
            url: "".into(),
        });
    }

    (blocks, toc)
}
