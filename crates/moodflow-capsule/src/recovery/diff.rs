//! Bounded line comparison with explicit fallback for large inputs.
use serde::Serialize;
#[derive(Serialize)]
pub struct Row {
    pub kind: &'static str,
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub text: String,
}
#[derive(Serialize)]
pub struct Comparison {
    pub rows: Vec<Row>,
    pub added: usize,
    pub removed: usize,
    pub detailed: bool,
    pub truncated: bool,
    pub left_lines: usize,
    pub right_lines: usize,
}
pub fn compare(left: &str, right: &str) -> Comparison {
    let left_lines = left.split('\n').count();
    let right_lines = right.split('\n').count();
    let mut out = Comparison {
        rows: vec![],
        added: 0,
        removed: 0,
        detailed: true,
        truncated: false,
        left_lines,
        right_lines,
    };
    if left_lines > 4000 || right_lines > 4000 {
        out.detailed = false;
        out.truncated = true;
        return out;
    }
    let a = left.split('\n').collect::<Vec<_>>();
    let b = right.split('\n').collect::<Vec<_>>();
    let mut raw = Vec::new();
    if (a.len() + 1).saturating_mul(b.len() + 1) <= 500_000 {
        let cols = b.len() + 1;
        let mut lcs = vec![0_u16; (a.len() + 1) * cols];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i * cols + j] = if a[i] == b[j] {
                    1 + lcs[(i + 1) * cols + j + 1]
                } else {
                    lcs[(i + 1) * cols + j].max(lcs[i * cols + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                raw.push(("same", Some(i + 1), Some(j + 1), a[i]));
                i += 1;
                j += 1;
            } else if j < b.len()
                && (i == a.len() || lcs[i * cols + j + 1] > lcs[(i + 1) * cols + j])
            {
                raw.push(("added", None, Some(j + 1), b[j]));
                j += 1;
            } else {
                raw.push(("removed", Some(i + 1), None, a[i]));
                i += 1;
            }
        }
    } else {
        out.detailed = false;
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        for (i, line) in a[..prefix].iter().enumerate() {
            raw.push(("same", Some(i + 1), Some(i + 1), *line));
        }
        for (i, line) in a[prefix..a.len() - suffix].iter().enumerate() {
            raw.push(("removed", Some(prefix + i + 1), None, *line));
        }
        for (i, line) in b[prefix..b.len() - suffix].iter().enumerate() {
            raw.push(("added", None, Some(prefix + i + 1), *line));
        }
        for i in 0..suffix {
            raw.push((
                "same",
                Some(a.len() - suffix + i + 1),
                Some(b.len() - suffix + i + 1),
                a[a.len() - suffix + i],
            ));
        }
    }
    let mut visible = vec![false; raw.len()];
    for (index, row) in raw.iter().enumerate() {
        out.added += usize::from(row.0 == "added");
        out.removed += usize::from(row.0 == "removed");
        if row.0 != "same" {
            for keep in visible
                .iter_mut()
                .take((index + 4).min(raw.len()))
                .skip(index.saturating_sub(3))
            {
                *keep = true;
            }
        }
    }
    if out.added + out.removed == 0 {
        for keep in visible.iter_mut().take(40) {
            *keep = true;
        }
    }
    let mut gap = false;
    for ((kind, l, r, text), keep) in raw.into_iter().zip(visible) {
        if !keep {
            gap = true;
            continue;
        }
        if out.rows.len() + if gap { 2 } else { 1 } > 400 {
            out.truncated = true;
            break;
        }
        if gap {
            out.rows.push(Row {
                kind: "gap",
                left: None,
                right: None,
                text: String::new(),
            });
            gap = false;
        }
        let rendered = text.chars().take(512).collect::<String>();
        out.truncated |= rendered.len() < text.len();
        out.rows.push(Row {
            kind,
            left: l,
            right: r,
            text: rendered,
        });
    }
    out
}
