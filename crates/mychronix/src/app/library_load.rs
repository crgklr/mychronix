//! The library (tracks and sessions) loaded again in the background when it
//! changes: pages keep showing the one they have until the new one is ready,
//! so a reload never stalls the window. Only the first load happens on the
//! spot, when a page first needs it.

use super::*;

#[derive(Default)]
pub(super) struct LibraryLoad {
    /// Reloads asked for so far, and the newest one swapped in.
    wanted: u64,
    shown: u64,
    /// The reload running, with its number.
    running: Option<(u64, std::sync::mpsc::Receiver<Library>)>,
}

impl App {
    /// The tracks or sessions on disk changed: load them again in the background.
    pub(super) fn reload_library(&mut self) {
        self.library_load.wanted += 1;
    }

    /// Swaps in a finished reload, and starts the next when something changed
    /// since the last one started. Once a frame, after the pages draw.
    pub(super) fn library_poll(&mut self, ctx: &egui::Context) {
        let load = &mut self.library_load;
        if let Some((n, rx)) = &load.running {
            match rx.try_recv() {
                Ok(lib) => {
                    load.shown = *n;
                    load.running = None;
                    self.library = Some(lib);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => load.running = None,
            }
        }
        // Never loaded yet: the first page to need it loads it, fresh.
        if self.library.is_none() {
            load.shown = load.wanted;
            return;
        }
        if load.shown < load.wanted {
            let n = load.wanted;
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            crate::jobs::spawn("library", move || {
                let _ = tx.send(Library::load());
                ctx.request_repaint();
            });
            load.running = Some((n, rx));
        }
    }
}
