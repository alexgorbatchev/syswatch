//! Scroll windows for the selectable lists.
//!
//! A list's window has to be remembered from one frame to the next. Derived
//! from the selection alone, the selected row is pinned to the window's bottom
//! edge, so moving back up from the end of a long list scrolls everything
//! under a cursor that never leaves the last row.

use std::cell::Cell;

/// First row of a `visible`-row window over `count` rows.
///
/// Starts from the previous window `prev` and moves it only as far as needed
/// to bring `selected` into view, so a cursor moving inside the window moves
/// alone. The result is clamped so a shrinking list never leaves blank rows
/// below its last entry while earlier rows are scrolled off the top.
pub fn window_start(prev: usize, selected: usize, count: usize, visible: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let selected = selected.min(count - 1);
    let visible = visible.max(1);
    let start = if selected < prev {
        selected
    } else if selected >= prev + visible {
        selected + 1 - visible
    } else {
        prev
    };
    start.min(count.saturating_sub(visible))
}

/// A list's remembered window.
///
/// Only rendering knows how many rows fit, so the renderer moves the window.
/// `Cell` because rendering takes `&App` — the same reason Dense's graph
/// ceilings are cells.
#[derive(Debug, Clone, Default)]
pub struct ScrollWindow(Cell<usize>);

impl ScrollWindow {
    /// Move the window just enough to show `selected` and return its first row.
    pub fn follow(&self, selected: usize, count: usize, visible: usize) -> usize {
        let start = window_start(self.0.get(), selected, count, visible);
        self.0.set(start);
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_inside_the_window_leaves_it_alone() {
        // Window 40..50 of 50; the cursor walks up from the last row.
        for sel in 40..50 {
            assert_eq!(window_start(40, sel, 50, 10), 40, "sel {sel}");
        }
    }

    #[test]
    fn cursor_below_the_window_scrolls_it_down_just_far_enough() {
        assert_eq!(window_start(0, 10, 50, 10), 1);
        assert_eq!(window_start(0, 49, 50, 10), 40);
    }

    #[test]
    fn cursor_above_the_window_makes_it_the_first_row() {
        assert_eq!(window_start(40, 39, 50, 10), 39);
        assert_eq!(window_start(40, 0, 50, 10), 0);
    }

    #[test]
    fn shrinking_list_pulls_the_window_back() {
        // 50 rows became 45 while the window sat at 40..50.
        assert_eq!(window_start(40, 44, 45, 10), 35);
        // Fewer rows than fit: everything shows from the top.
        assert_eq!(window_start(40, 3, 5, 10), 0);
    }

    #[test]
    fn selection_past_the_end_is_treated_as_the_last_row() {
        assert_eq!(window_start(0, 99, 50, 10), 40);
    }

    #[test]
    fn empty_list_and_zero_height_do_not_panic() {
        assert_eq!(window_start(7, 3, 0, 10), 0);
        // No rows fit: the window still tracks the cursor as if one did.
        assert_eq!(window_start(0, 5, 50, 0), 5);
    }

    #[test]
    fn follow_remembers_the_window_between_calls() {
        let w = ScrollWindow::default();
        assert_eq!(w.follow(49, 50, 10), 40);
        assert_eq!(w.follow(46, 50, 10), 40);
        assert_eq!(w.follow(39, 50, 10), 39);
    }
}
