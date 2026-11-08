//! The news on the TV: the same source and format as the desktop launcher's "What's new" card
//! (`launcher/src/home.rs`, `backend::releases`, `github::json`): GitHub's release list of
//! Horizon's repository, read through Kira's public mirror when GitHub's anonymous rate limit
//! answers. The newest game release gives a title, a date and a few lines of its notes (the
//! desktop launcher shows the first line of 24 characters or more, cut at 140; the TV has room
//! for a few). The last answer is kept on disk so a headset that is offline still shows it.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// Horizon's repository, as `launcher/src/backend.rs::REPO`.
pub const REPO: &str = "horizon-rust/horizon";
/// Kira's read-only mirror of Horizon's release reads (`launcher/src/github.rs::MIRROR`).
pub const MIRROR: &str = "https://mcp.playhorizon.dev/github/public";
/// How many lines of the notes the TV shows.
pub const LINES: usize = 4;
/// A line is cut here, as `backend::notes_summary` does.
pub const LINE_CHARS: usize = 140;

/// What the idle page shows of the newest release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct News {
    /// The release's tag (`v0.9.0`, `nightly`).
    pub tag: String,
    pub title: String,
    /// `YYYY-MM-DD`, or empty.
    pub date: String,
    pub lines: Vec<String>,
}

/// Where the release list is read from; a test answers with a fixture.
pub trait NewsSource: Send + Sync {
    fn releases(&self) -> Result<Value, String>;
}

/// GitHub, then the mirror.
pub struct Web {
    pub user_agent: String,
}

impl Web {
    fn get(&self, url: &str) -> Result<Value, String> {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .user_agent(&self.user_agent)
            .build()
            .get(url)
            .set("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(code, _) => format!("{url} answered {code}"),
                ureq::Error::Transport(_) => "No connection".to_owned(),
            })?
            .into_json()
            .map_err(|e| e.to_string())
    }
}

impl NewsSource for Web {
    fn releases(&self) -> Result<Value, String> {
        let path = format!("/repos/{REPO}/releases?per_page=10");
        self.get(&format!("https://api.github.com{path}"))
            .or_else(|_| self.get(&format!("{MIRROR}{path}")))
    }
}

/// A line of a release's notes as plain text: no heading marks, bullets, emphasis, code ticks or
/// link targets (`[text](url)` keeps its text).
pub fn plain_line(line: &str) -> String {
    let line = line.trim();
    let line = line.trim_start_matches('#').trim_start();
    let line = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .unwrap_or(line);
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let (before, tail) = rest.split_at(open);
        let linked = tail
            .find("](")
            .and_then(|mid| Some((mid, tail[mid..].find(')')? + mid)));
        match linked {
            Some((mid, end)) => {
                out.push_str(before);
                out.push_str(&tail[1..mid]);
                rest = &tail[end + 1..];
            }
            None => {
                out.push_str(before);
                out.push('[');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace(['*', '`'], "")
        .replace("__", "")
        .trim()
        .to_owned()
}

/// The lines of a release body worth showing: plain text, at least 24 characters (the desktop
/// launcher's rule for its one line), not rules, tables or HTML comments, each cut at 140.
pub fn summary(body: &str, lines: usize) -> Vec<String> {
    let mut in_comment = false;
    let mut out = Vec::new();
    for raw in body.lines() {
        let raw = raw.trim();
        if raw.starts_with("<!--") {
            in_comment = !raw.contains("-->");
            continue;
        }
        if in_comment {
            in_comment = !raw.contains("-->");
            continue;
        }
        if raw.starts_with("---") || raw.starts_with('|') || raw.starts_with('<') {
            continue;
        }
        let line = plain_line(raw);
        if line.chars().count() <= 24 {
            continue;
        }
        out.push(match line.char_indices().nth(LINE_CHARS) {
            Some((i, _)) => format!("{}…", line[..i].trim_end()),
            None => line,
        });
        if out.len() == lines {
            break;
        }
    }
    out
}

/// The newest game release in GitHub's release list (drafts and the launcher's own `launcher@`
/// releases left out), as the idle page shows it.
pub fn from_releases(list: &Value) -> Option<News> {
    let release = list.as_array()?.iter().find(|r| {
        let tag = r["tag_name"].as_str().unwrap_or_default();
        !r["draft"].as_bool().unwrap_or(false) && !tag.is_empty() && !tag.starts_with("launcher")
    })?;
    let tag = release["tag_name"].as_str()?.to_owned();
    let name = release["name"].as_str().unwrap_or_default().trim();
    Some(News {
        title: if name.is_empty() {
            tag.clone()
        } else {
            name.to_owned()
        },
        tag,
        date: release["published_at"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect(),
        lines: summary(
            &release["body"]
                .as_str()
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            LINES,
        ),
    })
}

/// The news from `source`, kept in `cache` (the last good answer, read when the source fails).
pub fn load(source: &dyn NewsSource, cache: &Path) -> Result<News, String> {
    match source.releases().map(|list| from_releases(&list)) {
        Ok(Some(news)) => {
            if let Ok(json) = serde_json::to_vec(&news) {
                let _ = std::fs::write(cache, json);
            }
            Ok(news)
        }
        Ok(None) => Err("No release is published yet".into()),
        Err(e) => std::fs::read(cache)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_marks_are_taken_off_and_links_keep_their_text() {
        assert_eq!(
            plain_line("- **Sky** fixed: see [the notes](https://x.test/a) for `more`"),
            "Sky fixed: see the notes for more"
        );
        assert_eq!(plain_line("## What's new"), "What's new");
        assert_eq!(
            plain_line("an unclosed [bracket stays"),
            "an unclosed [bracket stays"
        );
    }

    #[test]
    fn a_summary_keeps_the_first_long_enough_lines_and_cuts_them() {
        let long = "x".repeat(200);
        let body = format!(
            "<!-- generated\nnot shown -->\n## Notes\n---\n| a | table |\n- short\n- The new room has a sofa and a fireplace\n\n{long}\nlast line that is long enough to show"
        );
        let lines = summary(&body, 2);
        assert_eq!(lines[0], "The new room has a sofa and a fireplace");
        assert_eq!(lines[1].chars().count(), 141);
        assert!(lines[1].ends_with('…'));
    }

    #[test]
    fn the_newest_game_release_gives_the_news_and_launcher_releases_are_skipped() {
        let list = json!([
            {"tag_name": "launcher@0.3.0", "name": "Launcher", "body": "A launcher release that is long enough to show"},
            {"tag_name": "draft-1", "draft": true, "body": "A draft release that is long enough to show"},
            {"tag_name": "nightly", "name": "Nightly build", "published_at": "2026-11-07T10:00:00Z",
             "body": "- The headset launcher shows this news line\r\n- A second line that is long enough"},
            {"tag_name": "v0.1.0", "name": "Old", "body": "An older release that is long enough to show"}
        ]);
        let news = from_releases(&list).unwrap();
        assert_eq!(news.tag, "nightly");
        assert_eq!(news.title, "Nightly build");
        assert_eq!(news.date, "2026-11-07");
        assert_eq!(news.lines.len(), 2);
        assert_eq!(from_releases(&json!([])), None);
    }

    struct Fixed(Result<Value, String>);
    impl NewsSource for Fixed {
        fn releases(&self) -> Result<Value, String> {
            self.0.clone()
        }
    }

    #[test]
    fn the_last_answer_is_shown_when_the_source_fails() {
        let dir = std::env::temp_dir().join(format!("quest-news-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("news.json");
        let list = json!([{"tag_name": "v1.0.0", "name": "One", "body": "The first release has a line long enough"}]);
        let first = load(&Fixed(Ok(list)), &cache).unwrap();
        let offline = load(&Fixed(Err("No connection".into())), &cache).unwrap();
        assert_eq!(first, offline);
        std::fs::remove_file(&cache).unwrap();
        assert_eq!(
            load(&Fixed(Err("No connection".into())), &cache),
            Err("No connection".into())
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
