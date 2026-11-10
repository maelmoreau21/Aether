//! Application state that does not depend on GPUI: navigation history, data
//! being loaded, library paging and the "Continue" row. The views only read it
//! and turn user actions into the transitions defined here.
use aether_client::{CatalogPage, CatalogQuery, RecentProgress, Work};

/// Page size of a library grid.
pub const LIBRARY_PAGE: usize = 60;
/// Entries of the "Continue" row.
pub const CONTINUE_LIMIT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryKind {
    Films,
    Series,
}

impl LibraryKind {
    /// The Core's catalogue `kind` filter.
    pub fn api_kind(self) -> &'static str {
        match self {
            Self::Films => "film",
            Self::Series => "series",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Films => "Films",
            Self::Series => "Séries",
        }
    }
}

/// A screen of the signed-in application. The player is not a route: it
/// covers the window above whatever screen started it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Home,
    Library(LibraryKind),
    /// A film or a series, by work id.
    Work(String),
}

/// Back/forward-less history: tabs (home, libraries) reset it, a work is pushed
/// on top of the screen that opened it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Navigator {
    stack: Vec<Route>,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            stack: vec![Route::Home],
        }
    }
}

impl Navigator {
    pub fn current(&self) -> &Route {
        // The stack is never empty: `back` keeps the root.
        self.stack.last().unwrap_or(&Route::Home)
    }
    /// Opens a top-level section, forgetting the pages opened from another one.
    pub fn tab(&mut self, route: Route) {
        self.stack.clear();
        self.stack.push(route);
    }
    /// Opens a page on top of the current one; opening the current page again
    /// does not stack a duplicate.
    pub fn push(&mut self, route: Route) {
        if self.current() != &route {
            self.stack.push(route);
        }
    }
    /// `false` when already at the root.
    pub fn back(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }
    pub fn can_go_back(&self) -> bool {
        self.stack.len() > 1
    }
    /// The section a page belongs to, to highlight it in the top bar.
    pub fn section(&self) -> &Route {
        self.stack.first().unwrap_or(&Route::Home)
    }
}

/// Something fetched from the Core.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Loadable<T> {
    /// Whether a request should start: never loaded, or failed before.
    pub fn needs_load(&self) -> bool {
        matches!(self, Self::Idle | Self::Failed(_))
    }
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// A library grid filled page by page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibraryState {
    pub items: Vec<Work>,
    pub has_more: bool,
    pub loading: bool,
    pub loaded_once: bool,
    pub error: Option<String>,
    pub total: Option<usize>,
}

impl LibraryState {
    /// The next page to request, or `None` while one is in flight or when the
    /// library is complete.
    pub fn next_query(&self, kind: LibraryKind) -> Option<CatalogQuery> {
        if self.loading || (self.loaded_once && !self.has_more) {
            return None;
        }
        Some(CatalogQuery {
            kind: Some(kind.api_kind().to_owned()),
            search: None,
            offset: self.items.len(),
            limit: LIBRARY_PAGE,
            ..CatalogQuery::default()
        })
    }
    pub fn start(&mut self) {
        self.loading = true;
        self.error = None;
    }
    /// Appends a page, ignoring works already listed (the catalogue may shift
    /// while it is browsed).
    pub fn apply(&mut self, page: CatalogPage) {
        self.loading = false;
        self.loaded_once = true;
        self.has_more = page.has_more;
        self.total = page.total.or(self.total);
        for work in page.items {
            if !self.items.iter().any(|known| known.id == work.id) {
                self.items.push(work);
            }
        }
    }
    pub fn fail(&mut self, error: String) {
        self.loading = false;
        self.error = Some(error);
    }
}

/// The "Continue" row: started, unfinished items, the most recent first, and
/// one episode per series (its latest).
pub fn continue_watching(recent: Vec<RecentProgress>) -> Vec<RecentProgress> {
    let mut rows: Vec<RecentProgress> = Vec::new();
    let mut sorted = recent;
    sorted.sort_by_key(|item| std::cmp::Reverse(item.updated_at));
    for item in sorted {
        if item.watched || item.position < 1.0 {
            continue;
        }
        if item.duration > 0.0 && item.position >= item.duration {
            continue;
        }
        let same_series =
            item.series_id.is_some() && rows.iter().any(|row| row.series_id == item.series_id);
        if same_series || rows.iter().any(|row| row.work_id == item.work_id) {
            continue;
        }
        rows.push(item);
        if rows.len() == CONTINUE_LIMIT {
            break;
        }
    }
    rows
}

/// "1 h 05", "42 min", "0:42" style durations for cards and the player.
pub fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// What is left of an item, for a "Continue" card.
pub fn remaining(position: f64, duration: f64) -> Option<String> {
    if duration.is_nan() || duration <= 0.0 || position >= duration {
        return None;
    }
    let minutes = ((duration - position) / 60.0).ceil() as u64;
    Some(if minutes >= 60 {
        format!("{} h {:02} restantes", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min restantes")
    })
}

/// Fraction watched, for progress bars.
pub fn fraction(position: f64, duration: f64) -> f32 {
    if duration > 0.0 && position.is_finite() {
        (position / duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(id: &str) -> Work {
        Work {
            id: id.into(),
            kind: "film".into(),
            title: id.into(),
            year: None,
            description: String::new(),
            poster_url: None,
        }
    }
    fn recent(id: &str, series: Option<&str>, position: f64, updated_at: i64) -> RecentProgress {
        RecentProgress {
            work_id: id.into(),
            title: id.into(),
            kind: "episode".into(),
            series_id: series.map(Into::into),
            position,
            duration: 1000.0,
            watched: false,
            updated_at,
        }
    }

    #[test]
    fn navigation_keeps_a_root_and_tabs_reset_the_history() {
        let mut nav = Navigator::default();
        assert_eq!(nav.current(), &Route::Home);
        assert!(!nav.back(), "the root stays");
        nav.tab(Route::Library(LibraryKind::Films));
        nav.push(Route::Work("w1".into()));
        nav.push(Route::Work("w1".into()));
        assert!(nav.can_go_back());
        assert_eq!(nav.section(), &Route::Library(LibraryKind::Films));
        assert!(nav.back());
        assert_eq!(nav.current(), &Route::Library(LibraryKind::Films));
        assert!(!nav.back(), "one push, one duplicate ignored");
        nav.push(Route::Work("w2".into()));
        nav.tab(Route::Home);
        assert_eq!(nav, Navigator::default());
    }

    #[test]
    fn library_pages_are_requested_once_and_merged() {
        let kind = LibraryKind::Series;
        let mut library = LibraryState::default();
        let first = library.next_query(kind).unwrap();
        assert_eq!(
            (first.kind.as_deref(), first.offset, first.limit),
            (Some("series"), 0, LIBRARY_PAGE)
        );
        library.start();
        assert_eq!(library.next_query(kind), None, "one request at a time");
        library.apply(CatalogPage {
            items: vec![work("a"), work("b")],
            has_more: true,
            total: Some(3),
        });
        assert_eq!(library.next_query(kind).unwrap().offset, 2);
        library.start();
        library.fail("network".into());
        assert_eq!(library.error.as_deref(), Some("network"));
        assert_eq!(library.next_query(kind).unwrap().offset, 2, "retry");
        library.start();
        library.apply(CatalogPage {
            items: vec![work("b"), work("c")],
            has_more: false,
            total: None,
        });
        let ids: Vec<_> = library.items.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(library.total, Some(3));
        assert_eq!(library.next_query(kind), None, "complete");
    }

    #[test]
    fn continue_row_keeps_started_unfinished_items_one_per_series() {
        let mut finished = recent("done", None, 999.0, 50);
        finished.watched = true;
        let mut at_end = recent("end", None, 1000.0, 60);
        at_end.kind = "film".into();
        let rows = continue_watching(vec![
            recent("film", None, 300.0, 10),
            recent("s1e1", Some("s1"), 400.0, 20),
            recent("s1e2", Some("s1"), 100.0, 30),
            recent("untouched", None, 0.0, 40),
            finished,
            at_end,
        ]);
        let ids: Vec<_> = rows.iter().map(|r| r.work_id.as_str()).collect();
        assert_eq!(ids, ["s1e2", "film"]);
        let many = (0..30)
            .map(|i| recent(&format!("w{i}"), None, 10.0, i))
            .collect();
        assert_eq!(continue_watching(many).len(), CONTINUE_LIMIT);
    }

    #[test]
    fn loadable_and_time_formatting() {
        assert!(Loadable::<()>::Idle.needs_load());
        assert!(Loadable::<()>::Failed("x".into()).needs_load());
        assert!(!Loadable::<()>::Loading.needs_load());
        assert_eq!(Loadable::Ready(3).ready(), Some(&3));
        assert_eq!(clock(42.9), "0:42");
        assert_eq!(clock(3_905.0), "1:05:05");
        assert_eq!(clock(f64::NAN), "0:00");
        assert_eq!(remaining(0.0, 3_900.0).as_deref(), Some("1 h 05 restantes"));
        assert_eq!(remaining(100.0, 400.0).as_deref(), Some("5 min restantes"));
        assert_eq!(remaining(10.0, 0.0), None);
        assert_eq!(fraction(50.0, 200.0), 0.25);
        assert_eq!(fraction(500.0, 200.0), 1.0);
        assert_eq!(fraction(5.0, 0.0), 0.0);
    }
}
