//! The GPUI views. Everything here draws what the logic layer (`catalog`, `search`, `form`,
//! `jobs`) decides, and nothing is specific to any one module.

pub(crate) mod app;
pub(crate) mod bar;
pub(crate) mod chrome;
pub(crate) mod icons;
pub(crate) mod live;
pub(crate) mod panel;
pub(crate) mod theme;
pub(crate) mod widgets;

use gpui::{App, Entity, Global};

use crate::catalog::Catalog;
use crate::history::{Entry, History};

/// The recent jobs, shared by the bar and the app window so both update when a job finishes.
pub(crate) struct Recents {
    pub entries: Vec<Entry>,
    /// False until the history file has been read, so a page can show a placeholder, not "empty".
    pub loaded: bool,
}

/// What the file index is doing, as an entity so the Find page and the bar redraw when it changes.
pub(crate) struct FindStatus {
    pub state: crate::finder::State,
}

/// What every view needs: the tools, the history, the live recents, and the file index.
#[derive(Clone)]
pub(crate) struct Hub {
    pub catalog: std::sync::Arc<Catalog>,
    pub history: History,
    pub recents: Entity<Recents>,
    pub finder: crate::finder::Finder,
    pub find_status: Entity<FindStatus>,
}

impl Global for Hub {}

impl Hub {
    /// Record a finished job in the history file and the live recents.
    pub(crate) fn record(&self, entry: Entry, cx: &mut App) {
        let history = self.history.clone();
        let e2 = entry.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = history.append(&e2);
            })
            .detach();
        self.recents.update(cx, |r, cx| {
            r.entries.insert(0, entry);
            r.entries.truncate(64);
            cx.notify();
        });
    }
}
