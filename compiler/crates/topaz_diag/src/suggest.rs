//! "Did you mean …?" suggestions — edit-distance over a candidate name set.
//!
//! Shared by the checker (unknown member/field, TPZ5006) and the resolver
//! (unknown export TPZ3009, unbound name TPZ5002) so every diagnostic suggests
//! from ONE place. Pure string functions: no type-system or AST dependency.

/// Optimal string alignment distance (Levenshtein plus adjacent transpositions),
/// so a single swapped pair like `lenght` ↔ `length` costs 1, not 2. Operates on
/// `char`s, so it is correct for non-ASCII names too.
fn osa_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut previous_previous = vec![0usize; m + 1];
    let mut previous: Vec<usize> = (0..=m).collect();
    let mut current = vec![0usize; m + 1];
    for i in 1..=n {
        current[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(previous_previous[j - 2] + 1);
            }
            current[j] = best;
        }
        std::mem::swap(&mut previous_previous, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[m]
}

/// The candidate closest to `target` by edit distance, when one is near enough to
/// be a plausible typo: edit distance ≤ `max(len)/3` (at least 1). Candidates
/// shorter than four characters are never suggested — a single edit away from a
/// three-letter member (say `set` from `get`) is too weak a signal, and pointing a
/// reader at an unrelated short member misleads more than it helps. An exact match
/// is skipped (it would have resolved, not reached here). Ties resolve to the first
/// candidate in lexicographic order. The tie-break is independent of the
/// source container's iteration order, including randomized hash maps.
///
/// Names beyond `MAX_SUGGEST_LEN` are skipped, and a candidate whose length alone
/// already exceeds the distance budget is rejected before the O(n·m) matrix is
/// built, so a pathological multi-kilobyte identifier (record fields are user
/// names) can never make the diagnostic path allocate an enormous matrix.
pub fn closest<'a>(target: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    // Below MIN a single edit is too ambiguous to suggest; above MAX a name is not
    // a plausible typo and not worth an edit-distance matrix.
    const MIN_SUGGEST_LEN: usize = 4;
    const MAX_SUGGEST_LEN: usize = 64;
    let target_scalars: Vec<char> = target.chars().take(MAX_SUGGEST_LEN + 1).collect();
    let target_len = target_scalars.len();
    if target_len > MAX_SUGGEST_LEN {
        return None;
    }
    let mut best: Option<(&'a str, usize)> = None;
    for cand in candidates {
        if cand == target {
            continue;
        }
        let cand_scalars: Vec<char> = cand.chars().take(MAX_SUGGEST_LEN + 1).collect();
        let cand_len = cand_scalars.len();
        if !(MIN_SUGGEST_LEN..=MAX_SUGGEST_LEN).contains(&cand_len) {
            continue;
        }
        let threshold = (target_len.max(cand_len) / 3).max(1);
        // Edit distance is at least the length difference, so a candidate that far
        // from `target` cannot win — skip it before allocating the matrix.
        if target_len.abs_diff(cand_len) > threshold {
            continue;
        }
        let dist = osa_distance(&target_scalars, &cand_scalars);
        if dist <= threshold
            && best.is_none_or(|(current, current_dist)| {
                dist < current_dist || (dist == current_dist && cand < current)
            })
        {
            best = Some((cand, dist));
        }
    }
    best.map(|(c, _)| c)
}

/// A `"; did you mean \`X\`?"` suffix for an unknown name diagnostic, or `""` when
/// no candidate is a plausible typo of `target`.
pub fn did_you_mean<'a>(target: &str, candidates: impl IntoIterator<Item = &'a str>) -> String {
    match closest(target, candidates) {
        Some(s) => format!("; did you mean `{s}`?"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The `Array` member set, as a stable, builtins-independent fixture.
    const MEMBERS: [&str; 3] = ["push", "get", "length"];
    fn members() -> impl Iterator<Item = &'static str> {
        MEMBERS.iter().copied()
    }

    #[test]
    fn closest_suggests_a_plausible_typo() {
        // transposition (the dogfooding case) and a single substitution / deletion.
        assert_eq!(closest("lenght", members()), Some("length"));
        assert_eq!(closest("lengh", members()), Some("length"));
        assert_eq!(closest("psuh", members()), Some("push"));
    }

    #[test]
    fn closest_stays_silent_for_unrelated_names() {
        // not close to push/get/length, and not a member at all.
        assert_eq!(closest("frobnicate", members()), None);
        assert_eq!(closest("pop", members()), None);
        // an exact (valid) member is never suggested back to itself.
        assert_eq!(closest("length", members()), None);
        // short members (`get`, 3 chars) are never suggested — `set` would
        // otherwise point a writer at the read-only accessor (misleading).
        assert_eq!(closest("set", members()), None);
        assert_eq!(closest("gett", members()), None);
    }

    #[test]
    fn did_you_mean_formats_only_on_a_hit() {
        assert_eq!(
            did_you_mean("lenght", members()),
            "; did you mean `length`?"
        );
        assert_eq!(did_you_mean("zzz", members()), "");
    }

    #[test]
    fn closest_uses_spelling_to_break_equal_distance_ties() {
        assert_eq!(closest("catt", ["cast", "cart"]), Some("cart"));
        assert_eq!(closest("catt", ["cart", "cast"]), Some("cart"));
    }

    #[test]
    fn closest_is_bounded_for_pathological_lengths() {
        // Names can be user identifiers, so a multi-kilobyte typo/candidate can reach
        // this path. It must yield no suggestion WITHOUT building a giant matrix
        // (length cap + length-difference prefilter), not OOM or hang.
        let huge = "x".repeat(20_000);
        assert_eq!(closest(&huge, members()), None);
        let huge2 = "y".repeat(20_000);
        assert_eq!(closest("width", [huge.as_str(), huge2.as_str()]), None);
    }
}
