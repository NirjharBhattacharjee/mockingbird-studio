//! The description step's `description/sheet.toml` and the limits YouTube
//! puts on it. Sources, checked when this was built:
//! - Title, description and tags:
//!   <https://developers.google.com/youtube/v3/docs/videos> (`snippet.*`)
//! - Chapters: <https://support.google.com/youtube/answer/9884579>

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::project::Result;
use crate::store;

pub const PATH: &str = "description/sheet.toml";

const TITLE_CHARS: usize = 100;
const DESCRIPTION_BYTES: usize = 5000;
const TAGS_CHARS: usize = 500;
const MIN_CHAPTERS: usize = 3;
const CHAPTER_SECONDS: u32 = 10;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub category: u32,
    pub made_for_kids: bool,
    pub altered_content: bool,
    #[serde(default, rename = "chapter")]
    pub chapters: Vec<Chapter>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
    /// Seconds into the film.
    pub at: u32,
    pub title: String,
}

pub fn read(dir: &Path) -> Result<Sheet> {
    store::read(&dir.join(PATH))
}

/// The description step's files must include the sheet, and the sheet
/// must pass `Sheet::check`.
pub fn check_submit(dir: &Path, paths: &[String]) -> Result<()> {
    if !paths.iter().any(|p| p == PATH) {
        return Err(format!("the description step needs {PATH}"));
    }
    read(dir)?.check()
}

/// `0:00`, or `1:02:03` from an hour on, as YouTube writes chapter times.
fn clock(s: u32) -> String {
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

impl Sheet {
    /// The description as it's posted: the chapter lines go under it.
    pub fn full_description(&self) -> String {
        if self.chapters.is_empty() {
            return self.description.clone();
        }
        let lines: Vec<_> = self
            .chapters
            .iter()
            .map(|c| format!("{} {}", clock(c.at), c.title))
            .collect();
        format!("{}\n\n{}", self.description.trim_end(), lines.join("\n"))
    }

    /// Tags counted YouTube's way: the commas between them count, and a tag
    /// with a space counts the quotes the API wraps it in.
    pub fn tags_chars(&self) -> usize {
        let quoted = |t: &String| t.chars().count() + if t.contains(' ') { 2 } else { 0 };
        self.tags.iter().map(quoted).sum::<usize>() + self.tags.len().saturating_sub(1)
    }

    /// Refuses a sheet YouTube would reject, naming the field.
    pub fn check(&self) -> Result<()> {
        let title = self.title.chars().count();
        if self.title.trim().is_empty() {
            return Err("title is empty".into());
        }
        if title > TITLE_CHARS {
            return Err(format!(
                "title is {title} characters; YouTube allows {TITLE_CHARS}"
            ));
        }
        let full = self.full_description();
        if full.len() > DESCRIPTION_BYTES {
            return Err(format!(
                "description is {} bytes with the chapter lines; YouTube allows {DESCRIPTION_BYTES}",
                full.len()
            ));
        }
        for (field, text) in [("title", &self.title), ("description", &full)] {
            if text.contains(['<', '>']) {
                return Err(format!("{field} has < or >, which YouTube refuses"));
            }
        }
        if self.tags_chars() > TAGS_CHARS {
            return Err(format!(
                "tags are {} characters with commas and quotes; YouTube allows {TAGS_CHARS}",
                self.tags_chars()
            ));
        }
        self.check_chapters()
    }

    // ponytail: the last chapter's length isn't checked; that needs the film's
    // duration, which the upload step can check when it has the film.
    fn check_chapters(&self) -> Result<()> {
        let c = &self.chapters;
        if c.is_empty() {
            return Ok(());
        }
        if c.len() < MIN_CHAPTERS {
            return Err(format!(
                "chapter: there are {}; YouTube needs at least {MIN_CHAPTERS}, or none",
                c.len()
            ));
        }
        if c[0].at != 0 {
            return Err(format!(
                "chapter 1 starts at {}; YouTube needs the first at 0:00",
                clock(c[0].at)
            ));
        }
        for (i, pair) in c.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            if b.at < a.at + CHAPTER_SECONDS {
                return Err(format!(
                    "chapter {} starts at {}, less than {CHAPTER_SECONDS} seconds after chapter {} at {}; \
                     chapters go in order and last {CHAPTER_SECONDS} seconds or more",
                    i + 2,
                    clock(b.at),
                    i + 1,
                    clock(a.at)
                ));
            }
        }
        if let Some(i) = c.iter().position(|ch| ch.title.trim().is_empty()) {
            return Err(format!("chapter {} has no title", i + 1));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Sheet {
        toml::from_str(
            r#"
title = "Example video"
description = """
What the video is about.
"""
tags = ["example", "two words"]
category = 28
made_for_kids = false
altered_content = false

[[chapter]]
at = 0
title = "Cold open"

[[chapter]]
at = 21
title = "What happened"

[[chapter]]
at = 3725
title = "Wrap"
"#,
        )
        .unwrap()
    }

    fn refused(edit: impl FnOnce(&mut Sheet)) -> String {
        let mut s = good();
        edit(&mut s);
        s.check().unwrap_err()
    }

    #[test]
    fn a_good_sheet_passes_and_joins_its_chapters() {
        let s = good();
        s.check().unwrap();
        assert_eq!(
            s.full_description(),
            "What the video is about.\n\n0:00 Cold open\n0:21 What happened\n1:02:05 Wrap"
        );
        // 7 + (9 + 2 quotes) + 1 comma
        assert_eq!(s.tags_chars(), 19);
    }

    #[test]
    fn each_limit_is_refused_naming_the_field() {
        let e = refused(|s| s.title = "x".repeat(101));
        assert!(e.starts_with("title is 101 characters"), "{e}");
        // 100 characters of 3 bytes each is still a good title.
        let mut s = good();
        s.title = "é".repeat(100);
        s.check().unwrap();

        let e = refused(|s| s.title = " ".into());
        assert!(e.starts_with("title is empty"), "{e}");

        let e = refused(|s| s.title = "a <b>".into());
        assert!(e.starts_with("title has < or >"), "{e}");

        // Under the limit alone, over it once the chapter lines are added.
        let e = refused(|s| s.description = "x".repeat(4990));
        assert!(e.starts_with("description is"), "{e}");
        assert!(e.contains("with the chapter lines"), "{e}");

        let e = refused(|s| s.chapters[1].title = "a > b".into());
        assert!(e.starts_with("description has < or >"), "{e}");

        // 50 tags of 9 characters and 49 commas: 499. One more space: 501.
        let mut s = good();
        s.tags = vec!["abcdefghi".into(); 50];
        assert_eq!(s.tags_chars(), 499);
        s.check().unwrap();
        s.tags[0] = "abcd efgh".into();
        let e = s.check().unwrap_err();
        assert!(e.starts_with("tags are 501 characters"), "{e}");
    }

    #[test]
    fn chapter_rules_are_refused() {
        let e = refused(|s| s.chapters.truncate(2));
        assert!(e.starts_with("chapter: there are 2"), "{e}");

        let e = refused(|s| s.chapters[0].at = 5);
        assert!(e.starts_with("chapter 1 starts at 0:05"), "{e}");

        let e = refused(|s| s.chapters[1].at = 9);
        assert!(e.starts_with("chapter 2 starts at 0:09"), "{e}");

        let e = refused(|s| s.chapters[2].at = 10);
        assert!(e.starts_with("chapter 3 starts at 0:10"), "{e}");

        let e = refused(|s| s.chapters[2].title = "".into());
        assert!(e.starts_with("chapter 3 has no title"), "{e}");

        let mut s = good();
        s.chapters.clear();
        s.check().unwrap();
    }
}
