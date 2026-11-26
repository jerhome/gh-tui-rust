//! Pieces shared by the screens that show a list loaded from GitHub (Repo list, PR list, and the next ones).

/// What a list screen has to show: the answer of its request.
pub(super) enum Content<T> {
    /// The first answer has not come yet.
    Loading,
    /// The items, in GitHub's order.
    Loaded(Vec<T>),
    /// The request failed: a message for the user.
    Failed(String),
}

impl<T> Content<T> {
    /// The loaded items; empty while loading or after a failure.
    pub(super) fn items(&self) -> &[T] {
        match self {
            Self::Loaded(items) => items,
            Self::Loading | Self::Failed(_) => &[],
        }
    }
}

/// The highlighted row of a list: an index kept within the rows, and the scrolling that keeps it visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Selection {
    index: usize,
}

impl Selection {
    /// The selected index (0 when there is no row).
    pub(super) fn index(self) -> usize {
        self.index
    }

    /// Moves to the previous row (stays on the first).
    pub(super) fn up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Moves to the next of `count` rows (stays on the last).
    pub(super) fn down(&mut self, count: usize) {
        if self.index + 1 < count {
            self.index += 1;
        }
    }

    /// Selects row `index` (the caller knows it exists).
    pub(super) fn set(&mut self, index: usize) {
        self.index = index;
    }

    /// Back to the first row.
    pub(super) fn reset(&mut self) {
        self.index = 0;
    }

    /// Brings the index back within `count` rows (the last one if the list got shorter).
    pub(super) fn clamp(&mut self, count: usize) {
        self.index = self.index.min(count.saturating_sub(1));
    }

    /// Index of the first row to draw in `height` rows so the selected one is visible (rows never wrap).
    pub(super) fn first_visible(self, height: usize) -> usize {
        (self.index + 1).saturating_sub(height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_stay_within_the_rows() {
        let mut selection = Selection::default();
        selection.up();
        assert_eq!(selection.index(), 0);
        selection.down(0);
        assert_eq!(selection.index(), 0);
        selection.down(2);
        selection.down(2);
        assert_eq!(selection.index(), 1);
        selection.up();
        assert_eq!(selection.index(), 0);
    }

    #[test]
    fn clamp_follows_a_shorter_list() {
        let mut selection = Selection::default();
        selection.set(5);
        selection.clamp(3);
        assert_eq!(selection.index(), 2);
        selection.clamp(0);
        assert_eq!(selection.index(), 0);
    }

    #[test]
    fn scrolls_only_when_the_selection_is_below_the_window() {
        let mut selection = Selection::default();
        assert_eq!(selection.first_visible(3), 0);
        selection.set(2);
        assert_eq!(selection.first_visible(3), 0);
        selection.set(3);
        assert_eq!(selection.first_visible(3), 1);
        selection.reset();
        assert_eq!(selection.first_visible(3), 0);
    }

    #[test]
    fn items_are_empty_unless_loaded() {
        assert!(Content::<u8>::Loading.items().is_empty());
        assert!(Content::<u8>::Failed("x".into()).items().is_empty());
        assert_eq!(Content::Loaded(vec![1u8]).items(), &[1]);
    }
}
