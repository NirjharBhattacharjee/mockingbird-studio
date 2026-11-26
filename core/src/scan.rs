//! The scan: fetch the feeds a niche lists, keep what's new, write
//! `~/.mockingbird-studio/scans/<niche>/<day>.json`. The only network use is
//! one GET per listed feed. A link inside a feed is stored, never fetched.
//! Ranking is not here: the agent or the configured command does that (M4-2).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::niche::{self, Niche};
use crate::ops::Args;
use crate::project::Result;
use crate::store;

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const WINDOW_HOURS: i64 = 48;
/// A link seen in a scan this many days back (or less) is not new.
const SEEN_DAYS: i64 = 7;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    pub link: String,
    pub feed: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<Timestamp>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FeedResult {
    pub url: String,
    /// Why this feed gave nothing. The others are still scanned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Entries with a web link, before the window and the seen links.
    pub entries: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Scan {
    pub niche: String,
    pub day: Date,
    pub at: Timestamp,
    pub feeds: Vec<FeedResult>,
    pub items: Vec<Item>,
    /// The shortlist. Filled by M4-2.
    #[serde(default)]
    pub topics: Vec<Value>,
}

/// One GET: a timeout, a size cap, a generic user agent, no cookies (ureq
/// keeps none without its `cookies` feature) and no other headers of ours.
/// A redirect is not followed: the address that was listed is the only one
/// fetched, so a moved feed is an error that says where it went.
fn fetch(url: &str) -> Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent("mockingbird-studio")
        .max_redirects(0)
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    if resp.status().is_redirection() {
        let to = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("an address it didn't give");
        return Err(format!("moved to {to}; update the feed"));
    }
    resp.body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

fn day_of(t: Timestamp) -> Date {
    TimeZone::system().to_datetime(t).date()
}

/// The entries of one feed that have a web link. Relative links are made
/// absolute against the feed's address.
fn parse(url: &str, bytes: &[u8]) -> Result<Vec<Item>> {
    let feed = feed_rs::parser::Builder::new()
        .base_uri(Some(url))
        .build()
        .parse(bytes)
        .map_err(|e| format!("not a feed: {e}"))?;
    Ok(feed
        .entries
        .into_iter()
        .filter_map(|e| {
            let l = e
                .links
                .iter()
                .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
                .or(e.links.first())?;
            // Only http and https links go on the list.
            let link = niche::feed(&l.href).ok()?;
            Some(Item {
                title: e.title.map_or(String::new(), |t| t.content.trim().into()),
                feed: url.into(),
                link,
                published: e
                    .published
                    .or(e.updated)
                    .and_then(|d| Timestamp::from_second(d.timestamp()).ok()),
            })
        })
        .collect())
}

fn scan_file(dir: &Path, day: Date) -> PathBuf {
    dir.join(format!("{day}.json"))
}

/// The days that have a scan file in `dir`, and their paths.
fn days(dir: &Path) -> Vec<(Date, PathBuf)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return vec![];
    };
    rd.filter_map(|e| {
        let p = e.ok()?.path();
        let day = p.file_stem()?.to_str()?.parse().ok()?;
        (p.extension()? == "json").then_some((day, p))
    })
    .collect()
}

fn read_scan(path: &Path) -> Result<Scan> {
    store::read_json(path)
}

/// Links in the scans of the last `SEEN_DAYS` days before `today`. Today's
/// own file is left out: a second scan today replaces it, so it must be able
/// to find the same items again.
fn seen(dir: &Path, today: Date) -> HashSet<String> {
    let from = today - SEEN_DAYS.days();
    days(dir)
        .into_iter()
        .filter(|(d, _)| *d >= from && *d < today)
        .filter_map(|(_, p)| read_scan(&p).ok())
        .flat_map(|s| s.items.into_iter().map(|i| i.link))
        .collect()
}

/// Scans one niche into `<root>/<id>/<day>.json`. `get` fetches a feed's
/// bytes; the real one is `fetch`, tests pass one that reads files.
/// If every feed fails nothing is written, so a scan with the network down
/// can't replace a good one from earlier today.
fn scan_niche(
    root: &Path,
    n: &Niche,
    now: Timestamp,
    window_hours: i64,
    get: &dyn Fn(&str) -> Result<Vec<u8>>,
) -> Result<Scan> {
    if n.feeds.is_empty() {
        return Err(format!("{} has no feeds", n.id));
    }
    let dir = niche_dir(root, &n.id)?;
    let today = day_of(now);
    let since = now - SignedDuration::from_hours(window_hours);
    let mut seen = seen(&dir, today);
    let (mut feeds, mut items) = (vec![], vec![]);
    for url in &n.feeds {
        let got = niche::feed(url)
            .and_then(|u| get(&u))
            .and_then(|b| parse(url, &b));
        match got {
            Ok(entries) => {
                feeds.push(FeedResult {
                    url: url.clone(),
                    error: None,
                    entries: entries.len(),
                });
                // An entry with no date can't be placed in the window, so it's kept.
                items.extend(entries.into_iter().filter(|i| {
                    i.published.is_none_or(|p| p >= since) && seen.insert(i.link.clone())
                }));
            }
            Err(error) => feeds.push(FeedResult {
                url: url.clone(),
                error: Some(error),
                entries: 0,
            }),
        }
    }
    if feeds.iter().all(|f| f.error.is_some()) {
        let why: Vec<_> = feeds
            .iter()
            .map(|f| format!("{}: {}", f.url, f.error.as_deref().unwrap_or_default()))
            .collect();
        return Err(format!(
            "no feed of {} could be read: {}",
            n.id,
            why.join("; ")
        ));
    }
    // Newest first; undated ones last.
    items.sort_by(|a, b| b.published.cmp(&a.published));
    let scan = Scan {
        niche: n.id.clone(),
        day: today,
        at: now,
        feeds,
        items,
        topics: vec![],
    };
    // ponytail: a second scan on the same day drops that day's shortlist
    // (M4-2); keep it there if people rescan after ranking.
    store::write_json(&scan_file(&dir, today), &scan)?;
    Ok(scan)
}

/// `<root>/<id>`, refusing an id that could leave `root`.
fn niche_dir(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        return Err(format!("{id:?} can't be used as a folder name"));
    }
    Ok(root.join(id))
}

fn scans_root() -> PathBuf {
    store::home().join("scans")
}

fn window_hours(c: &store::Config) -> Result<i64> {
    match c.rest.get("scan").and_then(|s| s.get("window_hours")) {
        None => Ok(WINDOW_HOURS),
        Some(v) => v
            .as_integer()
            .filter(|h| (1..=24 * 365).contains(h))
            .ok_or_else(|| {
                "[scan] window_hours should be a whole number of hours, 1 or more".into()
            }),
    }
}

/// The niche `key` names, or every niche when there's no key.
fn pick(c: &store::Config, key: Option<String>) -> Result<Vec<&Niche>> {
    match key {
        Some(k) => Ok(vec![&c.niche[niche::find(&c.niche, &k)?]]),
        None => Ok(c.niche.iter().collect()),
    }
}

pub(crate) fn scan(a: &Args) -> Result<Value> {
    let c = store::read_config()?;
    let hours = window_hours(&c)?;
    let one = a.opt("niche");
    let named = one.is_some();
    let (mut scans, mut errors) = (vec![], vec![]);
    for n in pick(&c, one)? {
        match scan_niche(&scans_root(), n, Timestamp::now(), hours, &fetch) {
            Ok(s) => scans.push(s),
            // Asked for one niche: say why it failed. Asked for all: the
            // others still run, and the failure is in the reply.
            Err(e) if named => return Err(e),
            Err(e) => errors.push(json!({"niche": n.id, "error": e})),
        }
    }
    Ok(json!({"scans": scans, "errors": errors}))
}

pub(crate) fn topics(a: &Args) -> Result<Value> {
    let day: Option<Date> = a
        .opt("day")
        .map(|d| {
            d.trim()
                .parse()
                .map_err(|_| format!("day should look like 2026-11-26, not {d:?}"))
        })
        .transpose()?;
    let c = store::read_config()?;
    let one = a.opt("niche");
    let named = one.is_some();
    let (mut scans, mut errors) = (vec![], vec![]);
    for n in pick(&c, one)? {
        let got = niche_dir(&scans_root(), &n.id).and_then(|dir| {
            let found = match day {
                Some(d) => days(&dir).into_iter().find(|(x, _)| *x == d),
                None => days(&dir).into_iter().max_by_key(|(d, _)| *d),
            };
            found.map(|(_, p)| read_scan(&p)).transpose()
        });
        match got {
            Ok(Some(s)) => scans.push(s),
            Ok(None) if named => return Err(format!("no scan of {} yet; run scan", n.id)),
            Ok(None) => {}
            // As with `scan`: one niche's bad file doesn't hide the others.
            Err(e) if named => return Err(e),
            Err(e) => errors.push(json!({"niche": n.id, "error": e})),
        }
    }
    Ok(json!({"scans": scans, "errors": errors}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn feed_file(name: &str) -> Vec<u8> {
        let p = format!("{}/tests/feeds/{name}", env!("CARGO_MANIFEST_DIR"));
        fs::read(p).unwrap()
    }

    /// Feeds come from the test files; any other address is unreachable.
    fn from_files(url: &str) -> Result<Vec<u8>> {
        match url {
            "https://news.example/rss.xml" => Ok(feed_file("rss.xml")),
            "https://blog.example/atom.xml" => Ok(feed_file("atom.xml")),
            "https://junk.example/feed" => Ok(b"<html>not a feed</html>".to_vec()),
            _ => Err("connection refused".into()),
        }
    }

    fn niche(feeds: &[&str]) -> Niche {
        let mut n: Niche = toml::from_str("id = \"ai-news\"\nname = \"AI news\"").unwrap();
        n.feeds = feeds.iter().map(|f| f.to_string()).collect();
        n
    }

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    const NOW: &str = "2026-11-26T12:00:00Z";
    const BOTH: [&str; 2] = [
        "https://news.example/rss.xml",
        "https://blog.example/atom.xml",
    ];

    #[test]
    fn rss_and_atom_are_read_and_the_window_applied() {
        let root = tempfile::tempdir().unwrap();
        let mut feeds = BOTH.to_vec();
        feeds.push("https://junk.example/feed");
        let s = scan_niche(root.path(), &niche(&feeds), at(NOW), 48, &from_files).unwrap();
        let got: Vec<_> = s
            .items
            .iter()
            .map(|i| (i.title.as_str(), i.link.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Newest post", "https://blog.example/newest"),
                ("Only updated", "https://blog.example/updated-only"),
                // The Atom copy of this story comes second and is dropped.
                ("Fresh story", "https://news.example/fresh"),
                ("Undated story", "https://news.example/undated"),
            ]
        );
        // Old and last-month are out; no link and a javascript: link never made the list.
        assert_eq!(s.items[2].feed, BOTH[0]);
        assert!(s.items[3].published.is_none());
        // A page that isn't a feed is an error on its own line; the rest still ran.
        assert_eq!(s.feeds.len(), 3);
        assert_eq!(s.feeds[0].entries, 3);
        assert_eq!(s.feeds[1].entries, 4);
        assert!(s.feeds[2].error.as_ref().unwrap().contains("not a feed"));
        assert!(s.feeds[0].error.is_none());
        // And it was written where `topics` looks.
        let back = read_scan(&scan_file(&root.path().join("ai-news"), s.day)).unwrap();
        assert_eq!(back.items, s.items);
    }

    #[test]
    fn a_wider_window_brings_the_old_ones_back() {
        let root = tempfile::tempdir().unwrap();
        let s = scan_niche(root.path(), &niche(&BOTH), at(NOW), 24 * 365, &from_files).unwrap();
        assert_eq!(s.items.len(), 6);
        assert!(s.items.iter().any(|i| i.link == "https://news.example/old"));
    }

    #[test]
    fn links_seen_in_the_last_week_are_dropped() {
        let root = tempfile::tempdir().unwrap();
        let n = niche(&BOTH);
        let wide = 24 * 365;
        // Scans 2 days ago and 10 days ago, with the window wide enough to hold everything.
        let two = scan_niche(
            root.path(),
            &n,
            at(NOW) - SignedDuration::from_hours(48),
            wide,
            &from_files,
        )
        .unwrap();
        assert_eq!(two.items.len(), 6);
        let dir = root.path().join("ai-news");
        // Move the old file 8 days further back: it is now outside the week.
        let old = scan_file(&dir, two.day);
        let far = scan_file(&dir, two.day - 8.days());
        fs::rename(&old, &far).unwrap();
        let s = scan_niche(root.path(), &n, at(NOW), wide, &from_files).unwrap();
        assert_eq!(s.items.len(), 6, "a scan older than a week doesn't count");
        // Put it back inside the week: everything in it is now seen.
        fs::rename(&far, &old).unwrap();
        let s = scan_niche(root.path(), &n, at(NOW), wide, &from_files).unwrap();
        assert!(s.items.is_empty(), "{:?}", s.items);
        // Today's own file doesn't count, so a second scan today finds them again.
        fs::remove_file(&old).unwrap();
        let first = scan_niche(root.path(), &n, at(NOW), wide, &from_files).unwrap();
        let again = scan_niche(root.path(), &n, at(NOW), wide, &from_files).unwrap();
        assert_eq!(first.items, again.items);
        assert_eq!(first.items.len(), 6);
    }

    #[test]
    fn when_every_feed_fails_the_earlier_scan_stays() {
        let root = tempfile::tempdir().unwrap();
        let good = scan_niche(root.path(), &niche(&BOTH), at(NOW), 48, &from_files).unwrap();
        let bad = niche(&["https://down.example/feed"]);
        let e = scan_niche(root.path(), &bad, at(NOW), 48, &from_files).unwrap_err();
        assert!(
            e.contains("down.example") && e.contains("connection refused"),
            "{e}"
        );
        let back = read_scan(&scan_file(&root.path().join("ai-news"), good.day)).unwrap();
        assert_eq!(back.items, good.items);
        assert!(scan_niche(root.path(), &niche(&[]), at(NOW), 48, &from_files).is_err());
        // A feed address that isn't http(s) is refused before anything is fetched.
        let never = |_: &str| -> Result<Vec<u8>> { panic!("fetched") };
        assert!(
            scan_niche(
                root.path(),
                &niche(&["file:///etc/passwd"]),
                at(NOW),
                48,
                &never
            )
            .is_err()
        );
    }

    #[test]
    fn a_niche_id_cannot_leave_the_scans_folder() {
        let root = Path::new("/scans");
        for bad in ["", "../x", "a/b", ".hidden", "a\\b"] {
            assert!(niche_dir(root, bad).is_err(), "{bad:?}");
        }
        assert_eq!(niche_dir(root, "ai-news").unwrap(), root.join("ai-news"));
    }

    /// Serves `body` once on a loopback port and returns the address and
    /// the request the client sent.
    fn serve(body: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
        serve_raw(head, body)
    }

    fn serve_raw(head: String, body: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/feed.xml", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut req = vec![0; 4096];
            let n = s.read(&mut req).unwrap();
            // The client may hang up early on a body that's too big.
            let _ = s
                .write_all(head.as_bytes())
                .and_then(|_| s.write_all(&body));
            String::from_utf8_lossy(&req[..n]).to_lowercase()
        });
        (url, h)
    }

    #[test]
    fn fetch_sends_a_generic_agent_and_nothing_else_of_ours() {
        let (url, h) = serve(feed_file("rss.xml"));
        let got = fetch(&url).unwrap();
        assert_eq!(got, feed_file("rss.xml"));
        let req = h.join().unwrap();
        assert!(req.starts_with("get /feed.xml"), "{req}");
        assert!(req.contains("user-agent: mockingbird-studio\r\n"), "{req}");
        for header in ["cookie", "authorization", "referer"] {
            assert!(!req.contains(header), "{req}");
        }
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let head = "HTTP/1.1 301 Moved Permanently\r\nLocation: https://elsewhere.example/feed\r\nContent-Length: 0\r\n\r\n";
        let (url, h) = serve_raw(head.into(), vec![]);
        let e = fetch(&url).unwrap_err();
        assert_eq!(
            e,
            "moved to https://elsewhere.example/feed; update the feed"
        );
        h.join().unwrap();
    }

    #[test]
    fn fetch_stops_at_five_megabytes() {
        let (url, h) = serve(vec![b'x'; MAX_BYTES as usize + 1]);
        let e = fetch(&url).unwrap_err();
        assert!(
            e.to_lowercase().contains("limit") || e.contains("exceed"),
            "{e}"
        );
        let _ = h.join();
    }
}
