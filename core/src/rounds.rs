//! Text rounds: a copy of each text output as it was submitted, kept in
//! `.studio/rounds/<step>/<round>/<path>`, and a line diff between rounds.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::project::{Kind, Output, Result, Step};

/// Above this, a round's text comes back without a diff.
const DIFF_BYTES: usize = 1_000_000;

fn round_dir(dir: &Path, step: &str, round: u32) -> PathBuf {
    dir.join(".studio/rounds")
        .join(step)
        .join(round.to_string())
}

/// Refuses `path` if its deepest existing part resolves outside the project
/// folder `dir`, as it would through a symlinked `.studio`. Checked before
/// anything under it is made or removed.
fn check_inside(dir: &Path, path: &Path) -> Result<()> {
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    let existing = path
        .ancestors()
        .find(|a| a.exists())
        .expect("the project folder exists");
    let real = existing.canonicalize().map_err(io)?;
    if !real.starts_with(dir.canonicalize().map_err(io)?) {
        return Err(format!("{} is outside the project folder", path.display()));
    }
    Ok(())
}

/// Copies the text outputs of `step`'s round `round` into `.studio`, in
/// place of anything that round kept before.
pub fn keep(dir: &Path, step: &str, round: u32, outputs: &[Output]) -> Result<()> {
    let io = |e: std::io::Error| format!("can't keep round {round} of {step}: {e}");
    let to_dir = round_dir(dir, step, round);
    check_inside(dir, &to_dir)?;
    match fs::remove_dir_all(&to_dir) {
        Err(e) if e.kind() != ErrorKind::NotFound => return Err(io(e)),
        _ => {}
    }
    for o in outputs.iter().filter(|o| o.kind == Kind::Text) {
        let to = to_dir.join(&o.path);
        let parent = to.parent().expect("a joined path has a parent");
        check_inside(dir, parent)?;
        fs::create_dir_all(parent).map_err(io)?;
        check_inside(dir, parent)?;
        match fs::remove_file(&to) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(io(e)),
            _ => {}
        }
        fs::copy(dir.join(&o.path), &to).map_err(io)?;
    }
    Ok(())
}

/// The kept copy of `rel` from `step`'s round `round`, or `None` if that
/// round kept none.
pub fn kept(dir: &Path, step: &str, round: u32, rel: &str) -> Result<Option<PathBuf>> {
    let path = round_dir(dir, step, round).join(rel);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
        Ok(_) => {}
    }
    check_inside(dir, &path)?;
    Ok(Some(path))
}

fn read(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The text of `rel` in `step`'s round `round` (the current one if `None`),
/// and what changed since the last earlier round that kept the same file.
/// `kept` is false when the round has no copy and the live file stands in.
pub fn text(dir: &Path, step: &Step, rel: &str, round: Option<u32>) -> Result<Value> {
    if !Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(format!("{rel} is not a path inside the project folder"));
    }
    let round = round.unwrap_or(step.round);
    let (text, is_kept) = match kept(dir, &step.id, round, rel)? {
        Some(path) => (read(&path)?, true),
        // A round submitted before rounds were kept: the live file is all there is.
        None if round == step.round && step.outputs.iter().any(|o| o.path == rel) => {
            (read(&dir.join(rel))?, false)
        }
        None => return Err(format!("no kept copy of {rel} in round {round}")),
    };
    let mut previous = None;
    for r in (1..round).rev() {
        if let Some(path) = kept(dir, &step.id, r, rel)? {
            previous = Some((r, read(&path)?));
            break;
        }
    }
    let mut v = json!({
        "step": step.id,
        "path": rel,
        "round": round,
        "kept": is_kept,
        "text": text,
        "previous_round": previous.as_ref().map(|(r, _)| r),
        "changes": null,
    });
    if let Some((_, old)) = &previous {
        if old.len().max(text.len()) > DIFF_BYTES {
            v["diff"] = json!("too large");
        } else {
            v["changes"] = json!(diff(old, &text));
        }
    }
    Ok(v)
}

/// A line diff from `old` to `new`: each line tagged ' ' kept, '-' gone or
/// '+' new, in reading order.
pub fn diff<'a>(old: &'a str, new: &'a str) -> Vec<(char, &'a str)> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..]
        .iter()
        .rev()
        .zip(b[pre..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (x, y) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut out: Vec<(char, &str)> = a[..pre].iter().map(|l| (' ', *l)).collect();
    let (n, m) = (x.len(), y.len());
    // ponytail: an O(n·m) LCS table over the changed middle. Past 4M cells
    // the middle shows as replaced whole; switch to Myers if that's hit.
    if n * m > 4_000_000 {
        out.extend(x.iter().map(|l| ('-', *l)));
        out.extend(y.iter().map(|l| ('+', *l)));
    } else {
        // lcs[i][j]: the longest common run of lines in x[i..] and y[j..].
        let mut lcs = vec![vec![0u32; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i][j] = if x[i] == y[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && x[i] == y[j] {
                out.push((' ', x[i]));
                (i, j) = (i + 1, j + 1);
            } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
                out.push(('-', x[i]));
                i += 1;
            } else {
                out.push(('+', y[j]));
                j += 1;
            }
        }
    }
    out.extend(a[a.len() - suf..].iter().map(|l| (' ', *l)));
    out
}

#[cfg(test)]
mod tests {
    use super::diff;

    #[test]
    fn diff_tags_kept_gone_and_new_lines() {
        let old = "title\nintro\nslow line\nend\n";
        let new = "title\nintro\nfast line\nnew point\nend\n";
        assert_eq!(
            diff(old, new),
            [
                (' ', "title"),
                (' ', "intro"),
                ('-', "slow line"),
                ('+', "fast line"),
                ('+', "new point"),
                (' ', "end"),
            ]
        );
        assert_eq!(diff("a\nb", "a\nb"), [(' ', "a"), (' ', "b")]);
        assert_eq!(diff("", "x"), [('+', "x")]);
        assert_eq!(diff("x\ny\nx", "x"), [(' ', "x"), ('-', "y"), ('-', "x")]);
    }
}
