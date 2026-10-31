//! Stable index ordering that stops at the first comparison error.

use std::cmp::Ordering;

pub(super) fn stable_order<E>(
    length: usize,
    mut compare: impl FnMut(usize, usize) -> Result<Ordering, E>,
) -> Result<Vec<usize>, E> {
    let mut order: Vec<usize> = (0..length).collect();
    let mut scratch = order.clone();
    let mut width = 1usize;
    while width < length {
        for start in (0..length).step_by(width.saturating_mul(2)) {
            let middle = start.saturating_add(width).min(length);
            let end = middle.saturating_add(width).min(length);
            let (mut left, mut right) = (start, middle);
            for destination in &mut scratch[start..end] {
                let take_left = right == end
                    || (left < middle
                        && compare(order[left], order[right])? != Ordering::Greater);
                if take_left {
                    *destination = order[left];
                    left += 1;
                } else {
                    *destination = order[right];
                    right += 1;
                }
            }
        }
        std::mem::swap(&mut order, &mut scratch);
        width = width.saturating_mul(2);
    }
    Ok(order)
}
