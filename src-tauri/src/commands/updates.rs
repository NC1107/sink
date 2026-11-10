//! In-app update check: compare the running build against the newest stable
//! GitHub release. Check only - installing stays with the package manager
//! (or a manual download): the app must not replace system packages, and a
//! quiet in-place write would bypass the checksums and attestations every
//! release carries.

use std::io::ErrorKind;
use std::process::{Command, Stdio};

use serde::Serialize;

const RELEASES_API: &str = "https://api.github.com/repos/NC1107/sink/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/NC1107/sink/releases/latest";

/// What the Settings screen shows: the running version, the newest stable
/// release when the feed was reachable, and where to fetch it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: String,
}

/// Small feed body, so read it whole instead of streaming: curl first, wget
/// as the fallback (Ubuntu desktop ships wget, not always curl). Shelling
/// out keeps a TLS stack out of the binary.
fn fetch_text(url: &str) -> Result<String, String> {
    let out = match Command::new("curl")
        .args([
            "-fsSL",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "15",
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Err(e) if e.kind() == ErrorKind::NotFound => Command::new("wget")
            // --tries=1: -T is per-attempt and wget retries 20x by default,
            // which would hang the check for minutes on a dead feed.
            .args([
                "-q",
                "--https-only",
                "--tries=1",
                "-T",
                "15",
                "-O-",
                "--header=Accept: application/vnd.github+json",
            ])
            .arg(url)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| {
                if e.kind() == ErrorKind::NotFound {
                    "checking needs curl or wget installed".to_string()
                } else {
                    e.to_string()
                }
            })?,
        Err(e) => return Err(e.to_string()),
        Ok(out) => out,
    };
    if !out.status.success() {
        return Err("the release feed refused the request".to_string());
    }
    String::from_utf8(out.stdout).map_err(|_| "the release feed was not text".to_string())
}

/// The release tag without its leading `v` (`v0.1.40` -> `0.1.40`). The
/// `/latest` endpoint already skips prereleases and drafts.
fn parse_latest_tag(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = value.get("tag_name")?.as_str()?;
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    // Only a plain dotted-number tag is comparable; anything else (empty,
    // suffixed, junk) means the feed is unusable, not "up to date".
    if tag.is_empty()
        || !tag
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(tag.to_string())
}

/// Numeric X.Y.Z comparison; a longer tail wins over a shorter equal head
/// (`1.2.1` > `1.2`), non-numeric parts count as zero.
fn is_newer(current: &str, latest: &str) -> bool {
    fn parts(version: &str) -> Vec<u64> {
        version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    let (mut from, mut to) = (parts(current), parts(latest));
    let width = from.len().max(to.len());
    from.resize(width, 0);
    to.resize(width, 0);
    to > from
}

fn check_blocking() -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = fetch_text(RELEASES_API)
        .ok()
        .and_then(|body| parse_latest_tag(&body));
    UpdateInfo {
        update_available: latest.as_deref().is_some_and(|v| is_newer(&current, v)),
        current_version: current,
        latest_version: latest,
        release_url: RELEASES_PAGE.to_string(),
    }
}

/// Never fails on a dead network: an unreachable feed reports
/// `latest_version: None` and the UI offers a retry. Off the UI thread, so
/// the `--max-time` bound never stalls a settings open.
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(check_blocking)
        .await
        .map_err(|e| e.to_string())
}

/// Fixed release URL only, so the frontend can't open arbitrary pages.
#[tauri::command]
pub fn open_release_page() -> Result<(), String> {
    super::open_url(RELEASES_PAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_tag_off_the_feed() {
        let body = r#"{"tag_name": "v0.1.40", "prerelease": false}"#;
        assert_eq!(parse_latest_tag(body), Some("0.1.40".to_string()));
    }

    #[test]
    fn parse_rejects_a_feed_without_a_tag() {
        assert_eq!(parse_latest_tag("{}"), None);
        assert_eq!(parse_latest_tag("not json"), None);
        assert_eq!(parse_latest_tag(r#"{"tag_name": 40}"#), None);
        assert_eq!(parse_latest_tag(r#"{"tag_name": ""}"#), None);
        assert_eq!(parse_latest_tag(r#"{"tag_name": "v"}"#), None);
        assert_eq!(parse_latest_tag(r#"{"tag_name": "v1.2.3-rc.1"}"#), None);
    }

    #[test]
    fn newer_means_a_higher_number_not_a_newer_string() {
        assert!(is_newer("0.1.9", "0.1.40"));
        assert!(is_newer("0.1.40", "0.2.0"));
        assert!(!is_newer("0.1.40", "0.1.40"));
        assert!(!is_newer("0.2.0", "0.1.40"));
        assert!(is_newer("1.2", "1.2.1"));
        assert!(!is_newer("1.2.1", "1.2"));
    }

    #[test]
    fn compare_survives_unreleased_shapes() {
        // Dev builds stamp 0.1.0: anything released is newer.
        assert!(is_newer("0.1.0", "0.1.40"));
        assert!(!is_newer("0.1.40", "oops"));
    }
}
