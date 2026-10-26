//! Which track each session belongs to: the one its MyChron recorded, or a
//! name you typed for it, matched to the tracks where it was recorded. When a
//! track arrives later where you'd named sessions, it's offered for them.

use std::collections::HashMap;
use std::path::Path;

use aim_track::LatLon;
use aim_xrk::SessionSummary;

use super::{Library, LocalSession, Track};

/// How far a session's start/finish may be from a track's to count as that track.
const SAME_PLACE_M: f64 = 1_000.0;
/// How far a track you named may be from where the session was recorded (a
/// track's point is its center; a big circuit's start/finish can be a
/// kilometer or two from it).
const NAMED_NEAR_M: f64 = 5_000.0;

/// Tracks that arrived where you'd named sessions after a track that wasn't
/// there yet, offered for them.
#[derive(Debug, Clone)]
pub struct TrackOffer {
    /// The name as you typed it.
    pub typed: String,
    /// Indices into [`Library::sessions`].
    pub sessions: Vec<usize>,
    /// Indices into [`Library::tracks`], the best guess first (see [`name_match`]).
    pub tracks: Vec<usize>,
}

/// How well `typed` names track `t`: 0 its very name (capitals aside), 1 the
/// same but for spaces and punctuation, 2 one inside the other ("kank" and
/// "Kankakee Sh"); None when they're unrelated.
pub fn name_match(typed: &str, t: &Track) -> Option<u8> {
    let squash = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let names = std::iter::once(t.name.as_str()).chain(t.full_name.as_deref());
    let typed_sq = squash(typed);
    names
        .filter_map(|n| {
            let n_sq = squash(n);
            if n.trim().eq_ignore_ascii_case(typed.trim()) {
                Some(0)
            } else if !typed_sq.is_empty() && n_sq == typed_sq {
                Some(1)
            } else if typed_sq.chars().count() >= 2 && (n_sq.contains(&typed_sq) || typed_sq.contains(&n_sq) && n_sq.chars().count() >= 2) {
                Some(2)
            } else {
                None
            }
        })
        .min()
}

impl Library {
    /// The tracks where a session was recorded (every configuration there),
    /// nearest first, with how far; none when its MyChron didn't place it.
    pub fn around(&self, s: &LocalSession) -> Vec<(usize, f64)> {
        s.place().map(|p| self.near(p, NAMED_NEAR_M)).unwrap_or_default()
    }

    /// Tracks within `within_m` of `place`, nearest first, with how far.
    pub fn near(&self, place: LatLon, within_m: f64) -> Vec<(usize, f64)> {
        let mut v: Vec<(usize, f64)> = self.tracks.iter().enumerate().filter_map(|(i, t)| Some((i, t.center?.distance_m(&place)))).filter(|(_, d)| *d <= within_m).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    /// A track by name, the one nearest where the session was recorded, and
    /// only one within a few kilometers of it (names repeat: an "ABC" in
    /// Wisconsin isn't yours). Without a place to compare, the name alone.
    pub fn match_by_name(&self, name: &str, s: &SessionSummary) -> Option<usize> {
        let place = s.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let distance = |t: &Track| match (place, t.center) {
            (Some(a), Some(b)) => a.distance_m(&b),
            _ => f64::MAX / 2.0,
        };
        (0..self.tracks.len())
            .filter(|&i| self.tracks[i].name.eq_ignore_ascii_case(name) || self.tracks[i].full_name.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(name)))
            .filter(|&i| distance(&self.tracks[i]) <= NAMED_NEAR_M || distance(&self.tracks[i]) == f64::MAX / 2.0)
            .min_by(|&a, &b| distance(&self.tracks[a]).total_cmp(&distance(&self.tracks[b])))
    }

    /// The track a session's MyChron recorded: by guid, else by name at the same place.
    pub(super) fn match_track(&self, s: &SessionSummary, by_guid: &HashMap<String, usize>) -> Option<usize> {
        if let Some(i) = s.track_guid.as_ref().and_then(|g| by_guid.get(g)) {
            return Some(*i);
        }
        let name = s.track.as_deref()?;
        let place = s.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let near = |t: &Track| match (place, t.center) {
            (Some(a), Some(b)) => a.distance_m(&b) < SAME_PLACE_M,
            _ => true,
        };
        self.tracks.iter().position(|t| t.name == name && near(t))
    }

    /// The track for session `session`, whose track you named `name` (in
    /// `meta`, saved beside `xrz`), as linked when you named it:
    /// - linked to a track: that one, while it's still here;
    /// - linked to none: none, and tracks that arrived here since are
    ///   offered, not used (see `App::track_offer_dialog`);
    /// - named before links were kept: a track with the very name here is
    ///   linked, as it already was; otherwise the tracks here now don't count
    ///   as new, except ones whose name is close to it (offered). The link is
    ///   saved.
    pub(super) fn link_named(&mut self, session: usize, name: &str, summary: &SessionSummary, meta: &mut crate::device::SessionMeta, xrz: &Path, by_guid: &HashMap<String, usize>) -> Option<usize> {
        let near = self.match_by_name(name, summary);
        let place = summary.start_finish.map(|(lat, lon)| LatLon { lat, lon });
        let around: Vec<usize> = place.map(|p| self.near(p, NAMED_NEAR_M).into_iter().map(|(i, _)| i).collect()).unwrap_or_default();
        match meta.track_guid.clone() {
            Some(g) if !g.is_empty() => by_guid.get(&g).copied().or(near),
            Some(_) => {
                let fresh: Vec<usize> = around.into_iter().filter(|&i| !meta.track_declined.contains(&self.tracks[i].guid)).collect();
                if !fresh.is_empty() {
                    self.offer(session, name, fresh);
                }
                None
            }
            None => {
                meta.track_guid = Some(near.map(|i| self.tracks[i].guid.clone()).unwrap_or_default());
                if near.is_none() {
                    meta.track_declined = around.iter().filter(|&&i| name_match(name, &self.tracks[i]).is_none()).map(|&i| self.tracks[i].guid.clone()).collect();
                }
                if let Err(e) = meta.save(xrz) {
                    log::warn!("couldn't save the track link for {}: {e}", xrz.display());
                }
                near
            }
        }
    }

    /// Adds `tracks` to the offer for sessions named `name`, with `session`.
    fn offer(&mut self, session: usize, name: &str, tracks: Vec<usize>) {
        match self.offers.iter_mut().find(|o| o.typed.eq_ignore_ascii_case(name)) {
            Some(o) => {
                o.sessions.push(session);
                for i in tracks {
                    if !o.tracks.contains(&i) {
                        o.tracks.push(i);
                    }
                }
            }
            None => self.offers.push(TrackOffer { typed: name.to_string(), sessions: vec![session], tracks }),
        }
    }

    /// Each offer's best guess first: the closest name, then the nearest.
    pub(super) fn rank_offers(&mut self) {
        for o in &mut self.offers {
            let place = o.sessions.first().and_then(|&i| self.sessions[i].place());
            let tracks = &self.tracks;
            let key = |&i: &usize| (name_match(&o.typed, &tracks[i]).unwrap_or(9), place.zip(tracks[i].center).map(|(a, b)| a.distance_m(&b) as i64).unwrap_or(i64::MAX));
            o.tracks.sort_by_key(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracks::Kind;

    fn track(name: &str, full: Option<&str>) -> Track {
        Track {
            name: name.into(),
            full_name: full.map(str::to_string),
            city: None,
            length_m: None,
            track_kind: aim_track::TrackKind::Unknown,
            surface: Vec::new(),
            id: String::new(),
            tkk: None,
            from_mychron: None,
            country: String::new(),
            region: String::new(),
            center: None,
            start_finish: None,
            guid: String::new(),
            kind: Kind::Aim,
            outline: Vec::new(),
            sessions: Vec::new(),
            sort_key: String::new(),
            search: String::new(),
        }
    }

    #[test]
    fn names_match_closest_first() {
        let short = track("Kankakee Sh", Some("Kankakee Short Course"));
        let full = track("Kankakee", None);
        assert_eq!(name_match("kankakee", &full), Some(0));
        assert_eq!(name_match("KANKAKEE SH", &short), Some(0));
        assert_eq!(name_match("kankakee-sh", &short), Some(1));
        assert_eq!(name_match("kank", &short), Some(2));
        assert_eq!(name_match("short", &short), Some(2));
        assert_eq!(name_match("Kankakee", &short), Some(2));
        assert_eq!(name_match("OVRP", &track("Kankakee", None)), None);
        // One letter matches too much to mean anything.
        assert_eq!(name_match("k", &short), None);
    }
}
