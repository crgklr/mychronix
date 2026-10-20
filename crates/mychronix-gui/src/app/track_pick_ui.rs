//! Changing which track a downloaded session belongs to (for a logger that
//! names the track differently, or not at all): saved in the session's
//! `.meta.json`, optionally for every session from that MyChron that day.

use super::*;

pub(super) struct TrackPick {
    path: PathBuf,
    choice: String,
    whole_day: bool,
}

impl App {
    /// The session's track as the library sees it (a chosen one wins).
    pub(super) fn session_track_name(&mut self, path: &std::path::Path) -> Option<String> {
        let lib = self.library.get_or_insert_with(Library::load);
        let s = lib.sessions.iter().find(|s| s.path == path)?;
        s.track.map(|t| lib.tracks[t].name.clone()).or_else(|| s.meta.as_ref().map(|m| m.track.clone()).filter(|t| !t.is_empty()))
    }

    pub(super) fn open_track_pick(&mut self, path: PathBuf) {
        let choice = self.session_track_name(&path).unwrap_or_default();
        self.track_pick = Some(TrackPick { path, choice, whole_day: true });
    }

    pub(super) fn track_pick_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pick) = self.track_pick.take() else {
            return;
        };
        let lib = self.library.get_or_insert_with(Library::load);
        let favorites = self.favorites.get_or_insert_with(crate::tracks::Favorites::load).clone();
        // Your tracks: custom ones, ones with sessions, and favorites.
        let mut names: Vec<String> = lib.tracks.iter().filter(|t| t.kind == crate::tracks::Kind::Custom || !t.sessions.is_empty() || favorites.contains(&t.guid)).map(|t| t.name.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        let this = lib.sessions.iter().find(|s| s.path == pick.path).map(|s| (s.logger.clone(), s.summary.date));
        let same_day: Vec<PathBuf> = match &this {
            Some((logger, Some(date))) => lib.sessions.iter().filter(|s| &s.logger == logger && s.summary.date == Some(*date)).map(|s| s.path.clone()).collect(),
            _ => vec![pick.path.clone()],
        };
        let (mut save, mut cancel) = (false, false);
        let closed = garage_manage_ui::look::dialog(ctx, "track_pick", garage_manage_ui::look::WIDTH, |ui| {
            use garage_manage_ui::look;
            look::header(ui, Icon::MapPin, SECONDARY, None, "Session track", "Which track this session was at. Laps, best times and track pages use it.");
            look::body(ui, |ui| {
                look::label(ui, "Track");
                look::dropdown(ui, "track_pick", &mut pick.choice, &names, "Choose a track", ui.available_width() - 16.0);
                if same_day.len() > 1 {
                    let date = this.as_ref().and_then(|t| t.1).map(crate::events::us_short).unwrap_or_default();
                    ui.add_space(8.0);
                    look::label(ui, "Change");
                    if look::option(ui, !pick.whole_day, None, "Just this session", "") {
                        pick.whole_day = false;
                    }
                    if look::option(ui, pick.whole_day, None, &format!("All {} sessions from this MyChron on {date}", same_day.len()), "They were all at the same track that day") {
                        pick.whole_day = true;
                    }
                }
            });
            let (l, r) = look::footer(ui, "Cancel", ("Save", Some(Icon::Check), look::Kind::Primary, !pick.choice.is_empty()));
            cancel = l;
            save = r;
        });
        let open = !closed && !cancel;
        if save {
            let targets = if pick.whole_day { same_day } else { vec![pick.path.clone()] };
            let mut failed = None;
            for p in &targets {
                let mut m = crate::device::SessionMeta::load(p).unwrap_or_default();
                m.track = pick.choice.clone();
                if let Err(e) = m.save(p) {
                    failed = Some(e.to_string());
                }
            }
            log::info!("track set to {} for {} session(s)", pick.choice, targets.len());
            if let Some(e) = failed {
                self.error = Some(format!("Couldn't save the track: {e}"));
            }
            self.library = None;
            return;
        }
        if open {
            self.track_pick = Some(pick);
        }
    }
}
