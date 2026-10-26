//! Which track a downloaded session belongs to: picked from the tracks where
//! it was recorded (every configuration there, narrowed as you type), or a
//! name of your own when its track isn't here yet. Saved in the session's
//! `.meta.json`, optionally for every session from that MyChron that day.
//! When a track later arrives where you named sessions, it's offered for them.

use super::*;
use crate::tracks::{Kind, Track, name_match};

/// What's picked: a track (by guid), or a name of your own.
#[derive(Clone, PartialEq)]
enum Choice {
    Track(String),
    Name(String),
}

pub(super) struct TrackPick {
    path: PathBuf,
    search: String,
    choice: Option<Choice>,
    whole_day: bool,
}

/// The offer on screen and what's picked in it: an index into its tracks,
/// or None to keep the sessions as they are.
pub(super) struct OfferPick {
    typed: String,
    pick: Option<usize>,
}

/// Choices a dialog lists, at most: more would make it scroll.
const SHOWN: usize = 4;

/// Where a track came from: "Alex's MyChron" (the driver of that MyChron's
/// sessions), "your MyChron", or "AiM's track list".
fn source(t: &Track, lib: &Library, profiles: &crate::profiles::Profiles) -> String {
    if t.kind != Kind::Custom {
        return "AiM's track list".into();
    }
    let driver = t.from_mychron.as_ref().and_then(|serial| {
        lib.sessions.iter().filter(|s| &s.logger == serial).max_by_key(|s| s.start()).map(|s| super::sessions_filter_ui::session_driver(s, profiles)).filter(|d| !d.is_empty())
    });
    match driver {
        Some(d) => format!("{d}'s MyChron"),
        None => "your MyChron".into(),
    }
}

/// "From Alex's MyChron · 1,022 ft Kart track". (Every track listed is at the
/// session's place, so how far each is would tell them apart only by noise.)
fn about(t: &Track, from: &str) -> String {
    let from = if from == "AiM's track list" { "AiM track".to_string() } else { format!("From {from}") };
    [Some(from), Some(t.description()).filter(|d| !d.is_empty())].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// The first letter in capitals.
fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl App {
    /// The session's track as the library sees it (see `LocalSession::track_name`).
    pub(super) fn session_track_name(&mut self, path: &std::path::Path) -> Option<String> {
        let lib = self.library.get_or_insert_with(Library::load);
        let s = lib.sessions.iter().find(|s| s.path == path)?;
        s.track_name(&lib.tracks)
    }

    pub(super) fn open_track_pick(&mut self, path: PathBuf) {
        let lib = self.library.get_or_insert_with(Library::load);
        let choice = lib.sessions.iter().find(|s| s.path == path).and_then(|s| match s.track {
            Some(t) => Some(Choice::Track(lib.tracks[t].guid.clone())),
            None => s.track_name(&lib.tracks).map(Choice::Name),
        });
        self.track_pick = Some(TrackPick { path, search: String::new(), choice, whole_day: true });
    }

    pub(super) fn track_pick_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pick) = self.track_pick.take() else {
            return;
        };
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let lib = self.library.get_or_insert_with(Library::load);
        let favorites = self.favorites.get_or_insert_with(crate::tracks::Favorites::load).clone();
        let Some(si) = lib.sessions.iter().position(|s| s.path == pick.path) else { return };
        let session = &lib.sessions[si];
        let around = lib.around(session);
        let distance = |i: usize| around.iter().find(|(j, _)| *j == i).map(|(_, d)| *d);
        // The list: the tracks here, nearest first, narrowed to what's typed
        // (closest names first). Where the MyChron didn't place the session,
        // your own tracks, ones you've raced and favorites.
        let search = pick.search.trim().to_string();
        let pool: Vec<usize> = if session.place().is_some() {
            around.iter().map(|(i, _)| *i).collect()
        } else {
            (0..lib.tracks.len()).filter(|&i| lib.tracks[i].kind == Kind::Custom || !lib.tracks[i].sessions.is_empty() || favorites.contains(&lib.tracks[i].guid)).collect()
        };
        let mut list: Vec<(usize, u8)> = pool.into_iter().filter_map(|i| if search.is_empty() { Some((i, 0)) } else { name_match(&search, &lib.tracks[i]).map(|q| (i, q)) }).collect();
        list.sort_by(|a, b| a.1.cmp(&b.1).then(distance(a.0).unwrap_or(f64::MAX).total_cmp(&distance(b.0).unwrap_or(f64::MAX))));
        let exact = list.iter().any(|&(_, q)| q == 0) && !search.is_empty();
        let mut list: Vec<usize> = list.into_iter().map(|(i, _)| i).collect();
        // A name of your own: the one typed, when no track has it, or the
        // session's current one.
        let free = match &pick.choice {
            _ if !search.is_empty() => (!exact).then(|| search.clone()),
            Some(Choice::Name(n)) => Some(n.clone()),
            _ => None,
        };
        let free_row = free.is_some();
        list.truncate(SHOWN - usize::from(free_row));
        // The current pick stays in view.
        if let Some(Choice::Track(g)) = &pick.choice {
            if let Some(i) = lib.tracks.iter().position(|t| &t.guid == g).filter(|i| !list.contains(i)) {
                if list.len() >= SHOWN - usize::from(free_row) {
                    list.pop();
                }
                list.insert(0, i);
            }
        }
        let this = (session.logger.clone(), session.summary.date);
        let same_day: Vec<PathBuf> = match &this {
            (logger, Some(date)) => lib.sessions.iter().filter(|s| &s.logger == logger && s.summary.date == Some(*date)).map(|s| s.path.clone()).collect(),
            _ => vec![pick.path.clone()],
        };
        let (mut save, mut cancel) = (false, false);
        let closed = garage_manage_ui::look::dialog(ctx, "track_pick", garage_manage_ui::look::WIDTH, |ui| {
            use garage_manage_ui::look;
            look::header(ui, Icon::MapPin, SECONDARY, None, "Session track", "Which track this session was at. Laps, best times and track pages use it.");
            look::body(ui, |ui| {
                look::label(ui, "Track");
                // Typing replaces a name picked before (it would be out of sight).
                if look::text(ui, &mut pick.search, "Search, or type a name", None).changed() && matches!(pick.choice, Some(Choice::Name(_))) {
                    pick.choice = None;
                }
                for &i in &list {
                    let t = &lib.tracks[i];
                    let on = pick.choice == Some(Choice::Track(t.guid.clone()));
                    if look::option(ui, on, None, &t.name, &about(t, &source(t, lib, &profiles))) {
                        pick.choice = Some(Choice::Track(t.guid.clone()));
                    }
                }
                if let Some(name) = &free {
                    let on = pick.choice == Some(Choice::Name(name.clone()));
                    if look::option(ui, on, None, &format!("Name it \u{201c}{name}\u{201d}"), "No track here by that name. If one arrives, you'll be asked.") {
                        pick.choice = Some(Choice::Name(name.clone()));
                    }
                }
                if list.is_empty() && free.is_none() {
                    look::note(ui, "No tracks here yet. Type the track's name.", DIM);
                }
                if same_day.len() > 1 {
                    let date = this.1.map(crate::events::us_short).unwrap_or_default();
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
            let (l, r) = look::footer(ui, "Cancel", ("Save", Some(Icon::Check), look::Kind::Primary, pick.choice.is_some()));
            cancel = l;
            save = r;
        });
        let open = !closed && !cancel;
        if save {
            let targets = if pick.whole_day { same_day } else { vec![pick.path.clone()] };
            let mut failed = None;
            for p in &targets {
                let mut m = crate::device::SessionMeta::load(p).unwrap_or_default();
                match &pick.choice {
                    Some(Choice::Track(g)) => {
                        m.track = lib.tracks.iter().find(|t| &t.guid == g).map(|t| t.name.clone()).unwrap_or_default();
                        m.track_guid = Some(g.clone());
                        m.track_declined.clear();
                    }
                    Some(Choice::Name(n)) => {
                        // The tracks here now were passed over: only ones that arrive later are offered.
                        m.track = n.clone();
                        m.track_guid = Some(String::new());
                        m.track_declined = lib.sessions.iter().find(|s| &s.path == p).map(|s| lib.around(s).into_iter().map(|(i, _)| lib.tracks[i].guid.clone()).collect()).unwrap_or_default();
                    }
                    None => continue,
                }
                if let Err(e) = m.save(p) {
                    failed = Some(e.to_string());
                }
            }
            log::info!("track set for {} session(s)", targets.len());
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

    /// Asks which track sessions you named before their track was here were
    /// at, now that tracks have arrived where they were recorded (a custom
    /// track copied from a MyChron, AiM's list updated). The closest name is
    /// picked to start with; keeping them as they are stops the asking.
    pub(super) fn track_offer_dialog(&mut self, ctx: &egui::Context) {
        if self.track_pick.is_some() {
            return;
        }
        let profiles = self.profiles.get_or_insert_with(crate::profiles::Profiles::load).clone();
        let Some(lib) = self.library.as_ref() else { return };
        let Some(offer) = lib.offers.iter().find(|o| !self.offers_closed.contains(&o.typed.to_lowercase())).cloned() else {
            self.offer_pick = None;
            return;
        };
        if self.offer_pick.as_ref().is_none_or(|p| !p.typed.eq_ignore_ascii_case(&offer.typed)) {
            self.offer_pick = Some(OfferPick { typed: offer.typed.clone(), pick: Some(0) });
        }
        let Some(mut state) = self.offer_pick.take() else { return };
        let n = offer.sessions.len();
        let mut days: Vec<aim_xrk::summary::Date> = offer.sessions.iter().filter_map(|&i| lib.sessions[i].summary.date).collect();
        days.sort_unstable();
        days.dedup();
        let when = match (days.first(), days.last()) {
            (Some(a), Some(b)) if a != b => format!(" from {} to {}", crate::events::us_short(*a), crate::events::us_short(*b)),
            (Some(a), _) => format!(" on {}", crate::events::us_short(*a)),
            _ => String::new(),
        };
        let tracks: Vec<usize> = offer.tracks.iter().copied().take(SHOWN).collect();
        let (mut later, mut save) = (false, false);
        // Plain words: where the best guess came from, and whether its name
        // is the one you typed (then it's a yes or no), or not (then a choice).
        let lead = &lib.tracks[tracks[0]];
        let from = source(lead, lib, &profiles);
        let same_name = name_match(&offer.typed, lead).is_some_and(|q| q <= 1);
        let them = if n == 1 { "a session".to_string() } else { format!("{n} sessions") };
        let title = if same_name { format!("Use the {} track from {from}?", lead.name) } else { format!("Which track {} at?", if n == 1 { "was this session" } else { "were these sessions" }) };
        let arrived = if lead.kind == Kind::Custom {
            format!("{} has a track named \u{201c}{}\u{201d} that wasn't on this computer, so Mychronix copied it.", capitalized(&from), lead.name)
        } else {
            format!("AiM's track list now has \u{201c}{}\u{201d} where you recorded them.", lead.name)
        };
        let ask = if same_name {
            format!("You named {them} \u{201c}{}\u{201d}{when}, before it was here. Assign {} to this track?{}", offer.typed, if n == 1 { "it" } else { "them" }, if tracks.len() > 1 { " Other tracks at the same place are listed too." } else { "" })
        } else {
            format!("You named {them} \u{201c}{}\u{201d}{when}. Pick the track {} at, or keep {} as {}.", offer.typed, if n == 1 { "it was" } else { "they were" }, if n == 1 { "it" } else { "them" }, if n == 1 { "it is" } else { "they are" })
        };
        let closed = garage_manage_ui::look::dialog(ctx, "track_offer", garage_manage_ui::look::WIDTH, |ui| {
            use garage_manage_ui::look;
            look::header(ui, Icon::MapPin, SECONDARY, None, &title, &format!("{arrived} {ask}"));
            look::body(ui, |ui| {
                for (k, &i) in tracks.iter().enumerate() {
                    let t = &lib.tracks[i];
                    if look::option(ui, state.pick == Some(k), None, &t.name, &about(t, &source(t, lib, &profiles))) {
                        state.pick = Some(k);
                    }
                }
                let keep = format!("{} the name \u{201c}{}\u{201d}, and {} offered again.", if n == 1 { "It keeps" } else { "They keep" }, offer.typed, if tracks.len() == 1 { "this track isn't" } else { "these tracks aren't" });
                if look::option(ui, state.pick.is_none(), None, if n == 1 { "Keep it as it is" } else { "Keep them as they are" }, &keep) {
                    state.pick = None;
                }
            });
            let (l, r) = look::footer(ui, "Not now", (if same_name { "Assign" } else { "Save" }, Some(Icon::Check), look::Kind::Primary, true));
            later = l;
            save = r;
        });
        if closed || later {
            self.offers_closed.insert(offer.typed.to_lowercase());
            return;
        }
        if !save {
            self.offer_pick = Some(state);
            return;
        }
        let chosen = state.pick.and_then(|k| tracks.get(k)).map(|&i| (lib.tracks[i].name.clone(), lib.tracks[i].guid.clone()));
        let offered: Vec<String> = offer.tracks.iter().map(|&i| lib.tracks[i].guid.clone()).collect();
        let paths: Vec<PathBuf> = offer.sessions.iter().map(|&i| lib.sessions[i].path.clone()).collect();
        let mut failed = None;
        for p in &paths {
            let mut m = crate::device::SessionMeta::load(p).unwrap_or_default();
            match &chosen {
                Some((name, guid)) => {
                    m.track = name.clone();
                    m.track_guid = Some(guid.clone());
                    m.track_declined.clear();
                }
                None => {
                    for g in &offered {
                        if !m.track_declined.contains(g) {
                            m.track_declined.push(g.clone());
                        }
                    }
                }
            }
            if let Err(e) = m.save(p) {
                failed = Some(e.to_string());
            }
        }
        match &chosen {
            Some((name, _)) => log::info!("used track {name} for {} session(s) named {}", paths.len(), offer.typed),
            None => log::info!("kept {} session(s) named {} as they are", paths.len(), offer.typed),
        }
        if let Some(e) = failed {
            self.error = Some(format!("Couldn't save the track: {e}"));
        }
        self.library = None;
    }
}
