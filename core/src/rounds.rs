//! Text rounds: a copy of each text output as it was submitted, kept in
//! `.studio/rounds/<step>/<round>/<path>`, and a line diff between rounds.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::project::{Kind, Output, Result, Step};

fn copy_path(dir: &Path, step: &str, round: u32, rel: &str) -> PathBuf {
    dir.join(".studio/rounds")
        .join(step)
        .join(round.to_string())
        .join(rel)
}

/// Copies the text outputs of `step`'s round `round` into `.studio`.
pub fn keep(dir: &Path, step: &str, round: u32, outputs: &[Output]) -> Result<()> {
    for o in outputs.iter().filter(|o| o.kind == Kind::Text) {
        let to = copy_path(dir, step, round, &o.path);
        let io = |e: std::io::Error| format!("can't keep {} for round {round}: {e}", o.path);
        fs::create_dir_all(to.parent().expect("a joined path has a parent")).map_err(io)?;
        fs::copy(dir.join(&o.path), &to).map_err(io)?;
    }
    Ok(())
}

/// The text of `rel` in `step`'s round `round` (the current one if `None`),
/// and what changed since the last earlier round that kept the same file.
pub fn text(dir: &Path, step: &Step, rel: &str, round: Option<u32>) -> Result<Value> {
    if !Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(format!("{rel} is not a path inside the project folder"));
    }
    let round = round.unwrap_or(step.round);
    let read = |r: u32| fs::read_to_string(copy_path(dir, &step.id, r, rel)).ok();
    let text = match read(round) {
        Some(t) => t,
        // A round submitted before rounds were kept: the live file is all there is.
        None if round == step.round && step.outputs.iter().any(|o| o.path == rel) => {
            fs::read_to_string(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?
        }
        None => return Err(format!("no kept copy of {rel} in round {round}")),
    };
    let previous = (1..round).rev().find_map(|r| read(r).map(|t| (r, t)));
    Ok(json!({
        "step": step.id,
        "path": rel,
        "round": round,
        "text": text,
        "previous_round": previous.as_ref().map(|(r, _)| r),
        "changes": previous.as_ref().map(|(_, old)| diff(old, &text)),
    }))
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
