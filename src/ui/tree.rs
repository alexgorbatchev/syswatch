//! Branch connectors for a visible depth-first process tree.

use crate::cpu::CpuRow;

pub(crate) fn icon(row: &CpuRow<'_>, expanded: bool) -> char {
    if row.icon == '\u{f07b}' && expanded {
        '\u{f07c}'
    } else {
        row.icon
    }
}

pub(crate) fn prefixes(rows: &[CpuRow<'_>]) -> Vec<String> {
    from_depths(&rows.iter().map(|row| row.depth).collect::<Vec<_>>())
        .into_iter()
        .zip(rows)
        .map(|(prefix, row)| {
            if row.process.is_some() {
                prefix.replace('─', "")
            } else {
                prefix
            }
        })
        .collect()
}

fn from_depths(depths: &[usize]) -> Vec<String> {
    // The next row at the same or a shallower depth determines whether a
    // branch has another sibling. Descendants do not affect that decision.
    let mut pending = Vec::new();
    let mut last = vec![true; depths.len()];
    for (index, &depth) in depths.iter().enumerate().rev() {
        while pending.last().is_some_and(|&next| next > depth) {
            pending.pop();
        }
        last[index] = pending.last() != Some(&depth);
        pending.push(depth);
    }

    let mut ancestors = Vec::new();
    depths
        .iter()
        .enumerate()
        .map(|(index, &depth)| {
            ancestors.truncate(depth);
            let mut prefix = String::new();
            if depth > 0 {
                for &is_last in ancestors.iter().skip(1) {
                    prefix.push_str(if is_last { "   " } else { "│  " });
                }
                prefix.push_str(if last[index] { "╰─ " } else { "├─ " });
            }
            ancestors.push(last[index]);
            prefix
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::from_depths;

    #[test]
    fn connects_siblings_and_nested_branches_without_connecting_roots() {
        assert_eq!(
            from_depths(&[0, 1, 2, 3, 2, 1, 2, 0, 1, 0]),
            [
                "",
                "├─ ",
                "│  ├─ ",
                "│  │  ╰─ ",
                "│  ╰─ ",
                "╰─ ",
                "   ╰─ ",
                "",
                "╰─ ",
                ""
            ]
        );
    }

    #[test]
    fn empty_and_collapsed_trees_have_no_connectors() {
        assert!(from_depths(&[]).is_empty());
        assert_eq!(from_depths(&[0, 0]), ["", ""]);
    }
}
