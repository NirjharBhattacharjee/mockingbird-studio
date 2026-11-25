//! Niches: `[[niche]]` in config.toml. A name, the feeds you listed, a
//! default workflow, a visibility and the channel it posts to. Feeds are
//! only stored here; the scan that fetches them is M4.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ops::Args;
use crate::project::Result;
use crate::store;

// ponytail: one built-in workflow, as in `Project::new`; templates come in M5.
const WORKFLOWS: [&str; 1] = ["explainer"];

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Private,
    Unlisted,
    Public,
}

impl FromStr for Visibility {
    type Err = String;
    fn from_str(v: &str) -> Result<Self> {
        match v.trim().to_lowercase().as_str() {
            "private" => Ok(Self::Private),
            "unlisted" => Ok(Self::Unlisted),
            "public" => Ok(Self::Public),
            other => Err(format!(
                "visibility should be one of private, unlisted, public, not {other:?}"
            )),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Niche {
    /// Filled from the name by `tidy` when a hand-written niche has none.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub feeds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    #[serde(default)]
    pub visibility: Visibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Keys this version doesn't know are kept.
    #[serde(flatten)]
    pub rest: toml::Table,
}

/// Gives every niche an id and a name, so one hand-written `[[niche]]` that
/// lacks them can't make the whole config unreadable. Called after reading.
pub(crate) fn tidy(niches: &mut [Niche]) {
    for (i, n) in niches.iter_mut().enumerate() {
        if n.id.is_empty() {
            n.id = Some(slug(&n.name))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("niche-{}", i + 1));
        }
        if n.name.is_empty() {
            n.name = n.id.clone();
        }
    }
}

/// "AI news" is `ai-news`.
fn slug(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-")
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

/// An http or https address with a host. The scheme is lower-cased.
pub(crate) fn feed(url: &str) -> Result<String> {
    let url = url.trim();
    let bad = || format!("{url:?} isn't an http or https address with a host");
    let (scheme, rest) = url.split_once("://").ok_or_else(bad)?;
    let scheme = scheme.to_lowercase();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    if !matches!(scheme.as_str(), "http" | "https")
        || host.is_empty()
        || url.contains(char::is_whitespace)
    {
        return Err(bad());
    }
    Ok(format!("{scheme}://{rest}"))
}

/// The index of the niche `key` names: its id, else its name in any case.
/// A name that fits more than one niche is refused.
pub(crate) fn find(niches: &[Niche], key: &str) -> Result<usize> {
    let k = key.trim();
    let mut hits: Vec<usize> = (0..niches.len()).filter(|&i| niches[i].id == k).collect();
    if hits.is_empty() {
        hits = (0..niches.len())
            .filter(|&i| niches[i].name.eq_ignore_ascii_case(k))
            .collect();
    }
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("no niche {k:?}")),
        many => Err(format!(
            "{} niches match {k:?}; use the id of one: {}",
            many.len(),
            many.iter()
                .map(|&i| niches[i].id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
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
    let vis = a
        .opt("visibility")
        .map_or(Ok(Visibility::default()), |v| v.parse())?;
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
    let vis = a
        .opt("visibility")
        .map(|v| v.parse::<Visibility>())
        .transpose()?;
    let wf = a.opt("workflow").map(|v| workflow(&v)).transpose()?;
    store::update_config(|c| {
        let at = find(&c.niche, &a.text("niche"))?;
        if let Some(name) = a.opt("name") {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err("a niche needs a name".into());
            }
            if let Some(o) = c
                .niche
                .iter()
                .enumerate()
                .find(|(i, o)| {
                    *i != at
                        && (o.name.eq_ignore_ascii_case(&name) || o.id.eq_ignore_ascii_case(&name))
                })
                .map(|(_, o)| o)
            {
                return Err(format!("{:?} is already the name or id of {}", name, o.id));
            }
            c.niche[at].name = name;
        }
        let n = &mut c.niche[at];
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
        let at = find(&c.niche, &a.text("niche"))?;
        Ok(json!({"removed": c.niche.remove(at).id}))
    })
}

pub(crate) fn feed_add(a: &Args) -> Result<Value> {
    let url = feed(&a.text("url"))?;
    store::update_config(|c| {
        let at = find(&c.niche, &a.text("niche"))?;
        let n = &mut c.niche[at];
        if !n.feeds.contains(&url) {
            n.feeds.push(url);
        }
        Ok(reply(n))
    })
}

pub(crate) fn feed_remove(a: &Args) -> Result<Value> {
    let url = a.text("url");
    store::update_config(|c| {
        let at = find(&c.niche, &a.text("niche"))?;
        let n = &mut c.niche[at];
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

    fn niche(id: &str, name: &str) -> Niche {
        toml::from_str(&format!("id = {id:?}\nname = {name:?}")).unwrap()
    }

    #[test]
    fn slugs_visibilities_and_feeds() {
        assert_eq!(slug(" AI news! "), "ai-news");
        assert_eq!(slug("!!"), "");
        assert_eq!("Public".parse::<Visibility>().unwrap(), Visibility::Public);
        assert!("secret".parse::<Visibility>().is_err());
        assert_eq!(Visibility::default(), Visibility::Private);
        assert_eq!(
            feed(" HTTPS://a.example/Feed.xml ").unwrap(),
            "https://a.example/Feed.xml"
        );
        for bad in [
            "file:///etc/passwd",
            "https://",
            "http:///x",
            "https://user@/x",
            "https://a b/c",
            "ftp://a.example",
        ] {
            assert!(feed(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_niche_without_an_id_gets_one() {
        let mut ns: Vec<Niche> = toml::from_str::<toml::Table>(
            "[[n]]\nname = \"AI news\"\n[[n]]\n[[n]]\nid = \"x\"\nvisibility = \"public\"",
        )
        .unwrap()["n"]
            .clone()
            .try_into()
            .unwrap();
        tidy(&mut ns);
        let got: Vec<_> = ns
            .iter()
            .map(|n| (n.id.as_str(), n.name.as_str()))
            .collect();
        assert_eq!(
            got,
            [("ai-news", "AI news"), ("niche-2", "niche-2"), ("x", "x")]
        );
        assert_eq!(ns[2].visibility, Visibility::Public);
    }

    #[test]
    fn find_refuses_a_name_that_fits_two() {
        let ns = [niche("a", "Same"), niche("b", "same"), niche("c", "Other")];
        assert_eq!(find(&ns, "c").unwrap(), 2);
        assert_eq!(find(&ns, "OTHER").unwrap(), 2);
        assert_eq!(find(&ns, "a").unwrap(), 0);
        let e = find(&ns, "SAME").unwrap_err();
        assert!(e.contains("2 niches match") && e.contains("a, b"), "{e}");
        assert!(find(&ns, "none").is_err());
    }
}
