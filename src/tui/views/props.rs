//! Parameter groups shared by the view renderers, so each `render_*` takes
//! a few named groups instead of a long positional argument list (where two
//! `bool`s or two `&mut usize`s can be swapped without the compiler noticing).

use crate::tui::app::state::TextSearchState;
use crate::watcher::ResourceState;
use std::collections::HashMap;

/// Data a view shows that loads asynchronously.
#[derive(Debug)]
pub struct Fetched<'a, T: ?Sized> {
    pub data: Option<&'a T>,
    pub loading: bool,
}

// Manual impls: derive would require `T: Clone`.
impl<T: ?Sized> Clone for Fetched<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Copy for Fetched<'_, T> {}

impl<'a, T: ?Sized> Fetched<'a, T> {
    pub fn new(data: Option<&'a T>, loading: bool) -> Self {
        Self { data, loading }
    }

    /// Nothing loaded yet, nothing loading.
    pub fn none() -> Self {
        Self::new(None, false)
    }
}

/// Scroll position and `/` search of a text view.
pub struct TextScroll<'a> {
    pub offset: &'a mut usize,
    pub search: &'a mut TextSearchState,
}

impl<'a> TextScroll<'a> {
    pub fn new(offset: &'a mut usize, search: &'a mut TextSearchState) -> Self {
        Self { offset, search }
    }
}

/// Cursor of a list view: the selected row and the first row drawn.
pub struct ListCursor<'a> {
    pub selected: usize,
    pub offset: &'a mut usize,
}

impl<'a> ListCursor<'a> {
    pub fn new(selected: usize, offset: &'a mut usize) -> Self {
        Self { selected, offset }
    }
}

/// The watched Flux resources a detail view looks its selection up in.
#[derive(Clone, Copy)]
pub struct Watched<'a> {
    /// The selected `kind:namespace:name` key.
    pub key: &'a Option<String>,
    pub state: &'a ResourceState,
    /// Full objects by key.
    pub objects: &'a HashMap<String, serde_json::Value>,
}
