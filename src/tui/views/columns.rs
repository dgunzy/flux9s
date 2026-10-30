//! Column widths for content-sized tables.
//!
//! A list sizes each column to its widest value (#309: across every row, not
//! just the page on screen, so scrolling doesn't move them). On a live list
//! values change under the user — a status word, a pod name, a replica count
//! — and each change would shift every column to its right.
//! [`StableWidths`] holds them still: while the same list is open, a column
//! is as wide as the widest it has been, so columns grow but never shrink
//! until another list (or a resize) takes the table.

/// A column's natural width: its widest value or its header, at most `cap`.
pub fn fit(header: &str, values: impl Iterator<Item = usize>, cap: usize) -> u16 {
    let widest = values
        .chain(std::iter::once(header.chars().count()))
        .max()
        .unwrap_or(0);
    u16::try_from(widest.min(cap)).unwrap_or(u16::MAX)
}

/// The widths a list has shown since it opened, kept by the app between
/// frames (views are stateless; this is the one piece of layout memory).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StableWidths {
    /// What the widths belong to: the list, its columns, and the screen
    /// width. Another identity starts over.
    identity: String,
    widths: Vec<u16>,
}

impl StableWidths {
    /// The widths to draw: `natural` (this frame's), never narrower than
    /// what this list showed before.
    pub fn hold(&mut self, identity: &str, natural: &[u16]) -> Vec<u16> {
        if self.identity != identity || self.widths.len() != natural.len() {
            self.identity = identity.to_string();
            self.widths = natural.to_vec();
        } else {
            for (kept, now) in self.widths.iter_mut().zip(natural) {
                *kept = (*kept).max(*now);
            }
        }
        self.widths.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_fit_the_widest_value_or_header_within_a_cap() {
        assert_eq!(fit("STATUS", [5, 2].into_iter(), 20), 6, "the header");
        assert_eq!(fit("NAME", [12].into_iter(), 20), 12);
        assert_eq!(fit("NAME", [80].into_iter(), 20), 20, "capped");
    }

    #[test]
    fn a_list_keeps_its_widest_columns_until_another_takes_the_table() {
        let mut widths = StableWidths::default();
        assert_eq!(widths.hold("app|120", &[11, 20]), [11, 20]);
        // `Reconciling` became `Ready`: STATUS doesn't shrink.
        assert_eq!(widths.hold("app|120", &[6, 20]), [11, 20]);
        // A longer name: NAME grows.
        assert_eq!(widths.hold("app|120", &[6, 24]), [11, 24]);
        // Another list, or a resize, starts over.
        assert_eq!(widths.hold("app|100", &[6, 20]), [6, 20]);
        assert_eq!(widths.hold("other|100", &[5, 9, 9]), [5, 9, 9]);
    }
}
