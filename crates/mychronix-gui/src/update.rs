//! "A new version is available": asks GitHub, at most once a day and in the
//! background, for the newest release, and the app offers what's new and a
//! download link. Nothing is installed automatically.
//!
//! Quiet when offline, when the repository isn't public yet, or with
//! `"update_check": false` in `~/Mychronix/settings.json`. The last answer is
//! cached in `update-check.json` in the cache folder, along with a version
//! you've dismissed (it isn't offered again; a newer one is).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A release newer than this build.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// "0.2.0".
    pub version: String,
    /// The release page, with the downloads.
    pub url: String,
    /// Its release notes (Markdown as written on GitHub).
    pub notes: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    /// Seconds since 1970 of the last check.
    checked: u64,
    latest: Option<Release>,
    #[serde(default)]
    dismissed: Option<String>,
}

fn cache_path() -> Option<PathBuf> {
    crate::paths::cache_dir().map(|d| d.join("update-check.json"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn enabled() -> bool {
    let setting = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Mychronix").join("settings.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("update_check").and_then(|w| w.as_bool()));
    setting != Some(false)
}

/// "v0.10.2" → (0, 10, 2); None if it isn't a version.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut p = core.split('.').map(|x| x.parse::<u32>().ok());
    Some((p.next()??, p.next().flatten().unwrap_or(0), p.next().flatten().unwrap_or(0)))
}

/// Whether `latest` is newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

/// GitHub's "latest release" JSON → a release.
fn parse_release(text: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if v.get("draft").and_then(|d| d.as_bool()) == Some(true) || v.get("prerelease").and_then(|d| d.as_bool()) == Some(true) {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?;
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        url: v.get("html_url")?.as_str()?.to_string(),
        notes: v.get("body").and_then(|b| b.as_str()).unwrap_or_default().to_string(),
    })
}

/// "crgklr/mychronix" from the package's repository URL.
fn repo() -> Option<String> {
    let url = env!("CARGO_PKG_REPOSITORY");
    let path = url.trim_end_matches('/').trim_end_matches(".git").split("github.com/").nth(1)?;
    Some(path.to_string())
}

fn fetch() -> Result<Option<Release>, String> {
    let repo = repo().ok_or("no GitHub repository")?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Mychronix/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let r = client
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| e.to_string())?;
    // 404: no releases yet, or the repository is private.
    if r.status().as_u16() == 404 {
        return Ok(None);
    }
    let text = r.error_for_status().map_err(|e| e.to_string())?.text().map_err(|e| e.to_string())?;
    Ok(parse_release(&text))
}

/// Checks in the background (or answers from today's cache); sends a release
/// only when it's newer than this build and not dismissed.
pub fn spawn(ctx: eframe::egui::Context) -> std::sync::mpsc::Receiver<Release> {
    let (tx, rx) = std::sync::mpsc::channel();
    if !enabled() {
        return rx;
    }
    std::thread::spawn(move || {
        let path = cache_path();
        let mut cache: Cache = crate::storage::load_or_default(path.clone());
        // MYCHRONIX_UPDATE_TEST=<release JSON file> pretends GitHub answered with it (development only).
        let test = std::env::var("MYCHRONIX_UPDATE_TEST").ok().and_then(|p| std::fs::read_to_string(p).ok());
        if test.is_some() || now().saturating_sub(cache.checked) > 24 * 3600 {
            let answer = match test {
                Some(t) => Ok(parse_release(&t)),
                None => fetch(),
            };
            match answer {
                Ok(latest) => {
                    log::info!("update check: latest release {:?}", latest.as_ref().map(|r| &r.version));
                    cache.checked = now();
                    cache.latest = latest;
                    if let Some(p) = &path {
                        let _ = crate::storage::save(p, &cache);
                    }
                }
                Err(e) => log::debug!("update check failed: {e}"),
            }
        }
        if let Some(r) = cache.latest {
            if is_newer(&r.version, env!("CARGO_PKG_VERSION")) && cache.dismissed.as_deref() != Some(r.version.as_str()) {
                let _ = tx.send(r);
                ctx.request_repaint();
            }
        }
    });
    rx
}

/// Don't offer `version` again.
pub fn dismiss(version: &str) {
    let path = cache_path();
    let mut cache: Cache = crate::storage::load_or_default(path.clone());
    cache.dismissed = Some(version.to_string());
    if let Some(p) = path {
        let _ = crate::storage::save(&p, &cache);
    }
}

/// Opens a web page in the system's browser.
pub fn open_url(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if let Err(e) = std::process::Command::new(opener).arg(url).spawn() {
        log::warn!("couldn't open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert_eq!(parse_version("v1.2.3-beta"), Some((1, 2, 3)));
    }

    #[test]
    fn reads_githubs_latest_release() {
        let r = parse_release(r#"{"tag_name":"v0.2.0","html_url":"https://github.com/x/y/releases/tag/v0.2.0","body":"New: weather","draft":false,"prerelease":false}"#).unwrap();
        assert_eq!((r.version.as_str(), r.notes.as_str()), ("0.2.0", "New: weather"));
        assert!(parse_release(r#"{"tag_name":"v0.3.0","html_url":"u","prerelease":true}"#).is_none());
        assert_eq!(repo().as_deref(), Some("crgklr/mychronix"));
    }
}
