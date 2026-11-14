struct Edit {
    start: usize,
    end: usize,
    lines: Vec<String>,
}
fn edits(base: &str, other: &str) -> Vec<Edit> {
    let lines: Vec<_> = other.split_inclusive('\n').collect();
    similar::TextDiff::from_lines(base, other)
        .ops()
        .iter()
        .filter(|op| op.tag() != similar::DiffTag::Equal)
        .map(|op| Edit {
            start: op.old_range().start,
            end: op.old_range().end,
            lines: lines[op.new_range()]
                .iter()
                .map(|line| (*line).into())
                .collect(),
        })
        .collect()
}
/// Conservatively merge disjoint line edits. Overlapping edits require a user decision.
pub(crate) fn merge(base: &str, current: &str, generated: &str) -> Option<String> {
    if current == base {
        return Some(generated.into());
    }
    if generated == base || current == generated {
        return Some(current.into());
    }
    let mut left = edits(base, current);
    let right = edits(base, generated);
    for a in &left {
        for b in &right {
            let overlap = if a.start == a.end || b.start == b.end {
                a.start <= b.end && b.start <= a.end
            } else {
                a.start < b.end && b.start < a.end
            };
            if overlap {
                return None;
            }
        }
    }
    left.extend(right);
    left.sort_by_key(|edit| (edit.start, edit.end));
    let base: Vec<_> = base.split_inclusive('\n').collect();
    let mut result = String::new();
    let mut cursor = 0;
    for edit in left {
        result.extend(base[cursor..edit.start].iter().copied());
        result.extend(edit.lines);
        cursor = edit.end;
    }
    result.extend(base[cursor..].iter().copied());
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::merge;
    #[test]
    fn retains_unrelated_host_changes() {
        assert_eq!(
            merge(
                "one\ntwo\nthree\n",
                "one\nuser\nthree\n",
                "new\none\ntwo\nthree\n"
            )
            .unwrap(),
            "new\none\nuser\nthree\n"
        );
    }
    #[test]
    fn overlapping_edits_require_an_explicit_choice() {
        assert!(merge("one\n", "user\n", "new\n").is_none());
    }
}
