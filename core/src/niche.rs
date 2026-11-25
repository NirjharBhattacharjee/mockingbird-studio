//! Niches: `[[niche]]` in config.toml. A name, the feeds you listed, a
//! default workflow, a visibility and the channel it posts to. Feeds are
//! only stored here; the scan that fetches them is M4.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ops::Args;
use crate::project::Result;
use crate::store;

pub const VISIBILITIES: [&str; 3] = ["private", "unlisted", "public"];
// ponytail: one built-in workflow, as in `Project::new`; templates come in M5.
const WORKFLOWS: [&str; 1] = ["explainer"];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Niche {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub feeds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(default = "private")]
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Keys this version doesn't know are kept.
    #[serde(flatten)]
    pub rest: toml::Table,
}

fn private() -> String {
    "private".into()
}

/// "AI news" is `ai-news`.
fn slug(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn visibility(v: &str) -> Result<String> {
    let v = v.trim().to_lowercase();
    if VISIBILITIES.contains(&v.as_str()) {
        Ok(v)
    } else {
        Err(format!(
            "visibility should be one of {}, not {v:?}",
            VISIBILITIES.join(", ")
        ))
    }
}

/// An empty value clears a text field.
fn text(v: &str) -> Option<String> {
    Some(v.trim().to_string()).filter(|s| !s.is_empty())
}

fn workflow(v: &str) -> Result<Option<String>> {
    match text(v) {
        Some(w) if !WORKFLOWS.contains(&w.as_str()) => Err(format!(
            "unknown workflow {w:?}; the built-in one is {}",
            WORKFLOWS.join(", ")
        )),
        w => Ok(w),
    }
}

fn feed(url: &str) -> Result<String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    match rest {
        Some(r) if !r.is_empty() && !r.contains(char::is_whitespace) => Ok(url.to_string()),
        _ => Err(format!("{url:?} isn't an http or https address")),
    }
}

fn find<'a>(niches: &'a mut [Niche], key: &str) -> Result<&'a mut Niche> {
    let k = key.trim();
    let at = niches
        .iter()
        .position(|n| n.id == k)
        .or_else(|| niches.iter().position(|n| n.name.eq_ignore_ascii_case(k)))
        .ok_or_else(|| format!("no niche {k:?}"))?;
    Ok(&mut niches[at])
}

fn reply(n: &Niche) -> Value {
    json!({"niche": n})
}

pub(crate) fn niches(_: &Args) -> Result<Value> {
    Ok(json!({"niches": store::read_config()?.niche}))
}

pub(crate) fn niche_add(a: &Args) -> Result<Value> {
    let name = a.text("name").trim().to_string();
    let id = slug(&name);
    if id.is_empty() {
        return Err("a niche needs a name with a letter or digit in it".into());
    }
    let vis = visibility(&a.opt("visibility").unwrap_or_else(private))?;
    let wf = workflow(&a.opt("workflow").unwrap_or_default())?;
    let mut feeds = Vec::new();
    for f in a.list("feeds") {
        let f = feed(&f)?;
        if !feeds.contains(&f) {
            feeds.push(f);
        }
    }
    store::update_config(|c| {
        if c.niche.iter().any(|n| n.id == id) {
            return Err(format!("there is already a niche {id:?}"));
        }
        let n = Niche {
            id,
            name,
            feeds,
            workflow: wf,
            visibility: vis,
            channel: a.opt("channel").and_then(|v| text(&v)),
            rest: Default::default(),
        };
        let out = reply(&n);
        c.niche.push(n);
        Ok(out)
    })
}

pub(crate) fn niche_set(a: &Args) -> Result<Value> {
    let vis = a.opt("visibility").map(|v| visibility(&v)).transpose()?;
    let wf = a.opt("workflow").map(|v| workflow(&v)).transpose()?;
    store::update_config(|c| {
        let n = find(&mut c.niche, &a.text("niche"))?;
        if let Some(name) = a.opt("name") {
            if name.trim().is_empty() {
                return Err("a niche needs a name".into());
            }
            n.name = name.trim().to_string();
        }
        if let Some(v) = vis {
            n.visibility = v;
        }
        if let Some(w) = wf {
            n.workflow = w;
        }
        if let Some(ch) = a.opt("channel") {
            n.channel = text(&ch);
        }
        Ok(reply(n))
    })
}

pub(crate) fn niche_remove(a: &Args) -> Result<Value> {
    store::update_config(|c| {
        let id = find(&mut c.niche, &a.text("niche"))?.id.clone();
        c.niche.retain(|n| n.id != id);
        Ok(json!({"removed": id}))
    })
}

pub(crate) fn feed_add(a: &Args) -> Result<Value> {
    let url = feed(&a.text("url"))?;
    store::update_config(|c| {
        let n = find(&mut c.niche, &a.text("niche"))?;
        if !n.feeds.contains(&url) {
            n.feeds.push(url);
        }
        Ok(reply(n))
    })
}

pub(crate) fn feed_remove(a: &Args) -> Result<Value> {
    let url = a.text("url");
    store::update_config(|c| {
        let n = find(&mut c.niche, &a.text("niche"))?;
        let before = n.feeds.len();
        n.feeds.retain(|f| f != url.trim());
        if n.feeds.len() == before {
            return Err(format!("{:?} isn't a feed of {}", url.trim(), n.id));
        }
        Ok(reply(n))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_visibilities_and_feeds() {
        assert_eq!(slug(" AI news! "), "ai-news");
        assert_eq!(slug("!!"), "");
        assert_eq!(visibility("Public").unwrap(), "public");
        assert!(visibility("secret").is_err());
        assert!(feed("file:///etc/passwd").is_err());
        assert!(feed("https://a.example/feed.xml").is_ok());
    }
}
