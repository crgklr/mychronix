//! The Tracks page: AiM's track database and your custom tracks, by
//! country, with each track's map, splits, braking points and sessions.

use super::*;

impl App {
    pub(super) fn tracks_page(&mut self, ui: &mut Ui) {
        let mut lib = self.library.take().unwrap_or_else(Library::load);
        // Full names, cities and outlines arrive from a background reader.
        if self.track_details.is_none() {
            self.track_details = Some(tracks::load_details(lib.pending_details(), ui.ctx().clone()));
        }
        if let Some(rx) = &self.track_details {
            let mut got = false;
            while let Ok((i, d)) = rx.try_recv() {
                lib.apply_details(i, d);
                got = true;
            }
            if got {
                ui.ctx().request_repaint();
            }
        }
        if self.track_sel.is_none() && !lib.tracks.is_empty() {
            // MYCHRONIX_TRACK=<name> preselects a track (for screenshots).
            match std::env::var("MYCHRONIX_MAP").as_deref() {
                Ok("splits") => self.track_map = MapMode::Splits,
                Ok("elevation") => self.track_map = MapMode::Elevation,
                _ => {}
            }
            if std::env::var("MYCHRONIX_SATELLITE").is_ok() {
                self.map_satellite = true;
            }
            let wanted = std::env::var("MYCHRONIX_TRACK").ok();
            let i = wanted.and_then(|n| lib.tracks.iter().position(|t| t.name == n)).unwrap_or(0);
            self.select_track(&lib, i);
        }

        let visited = lib.tracks.iter().filter(|t| !t.sessions.is_empty()).count();
        ui.label(RichText::new("Tracks").font(semibold(28.0)).color(TEXT));
        ui.label(
            RichText::new(format!(
                "{} AiM tracks  ·  {} custom  ·  {} with your sessions",
                group_thousands(lib.count(Kind::Aim)),
                lib.count(Kind::Custom),
                visited
            ))
            .font(regular(14.5))
            .color(SECONDARY),
        );
        ui.add_space(18.0);
        self.aim_tracks_banner(ui, &lib);

        if lib.tracks.is_empty() {
            // The banner above already says why (downloading, or offline).
            if self.aim_tracks_progress.is_none() && !self.aim_tracks_offline {
                card(ui, |ui| {
                    ui.label(RichText::new("No tracks yet").font(semibold(17.0)).color(TEXT));
                    ui.label(RichText::new("AiM's track database downloads automatically when Mychronix is online, and tracks learned on your MyChron are copied when you connect it.").color(SECONDARY));
                });
            }
            self.library = Some(lib);
            return;
        }

        let gap = 14.0;
        let height = ui.available_height();
        let list_w = (ui.available_width() * 0.36).clamp(280.0, 380.0);
        let detail_w = ui.available_width() - list_w - gap;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let column = Layout::top_down(Align::Min);
            ui.allocate_ui_with_layout(vec2(list_w, height), column, |ui| {
                ui.set_width(list_w);
                self.track_list(ui, &lib);
            });
            ui.allocate_ui_with_layout(vec2(detail_w, height), column, |ui| {
                ui.set_width(detail_w);
                if let Some(i) = self.track_sel {
                    // The detail scrolls as one column; the map keeps a fixed share of the view.
                    let map_h = (height * 0.5).clamp(300.0, 620.0);
                    let mut scroll = egui::ScrollArea::vertical().id_salt("track_detail").auto_shrink([false, false]);
                    // MYCHRONIX_DETAIL_SCROLL=<px> starts the detail scrolled down (for screenshots).
                    if self.frames < 2 {
                        if let Some(px) = std::env::var("MYCHRONIX_DETAIL_SCROLL").ok().and_then(|v| v.parse().ok()) {
                            scroll = scroll.vertical_scroll_offset(px);
                        }
                    }
                    scroll.show(ui, |ui| {
                        self.track_detail(ui, &lib, i, map_h);
                    });
                }
            });
        });
        self.library = Some(lib);
    }

    fn select_track(&mut self, lib: &Library, i: usize) {
        self.track_sel = Some(i);
        let t = &lib.tracks[i];
        self.track_file = t.tkk.as_ref().and_then(|p| aim_track::read_tkk(p).ok()).map(|f| (i, f));
        if let Some(sf) = t.start_finish {
            self.map_memory.center_at(walkers::lat_lon(sf.lat, sf.lon));
            let _ = self.map_memory.set_zoom(16.5);
        }
        self.fit_map = true;
        // Your fastest lap here, for the speed map.
        if !t.sessions.is_empty() && !self.speed_laps.contains_key(&t.guid) {
            self.speed_laps.insert(t.guid.clone(), speed_lap(lib, t));
        }
    }

    fn track_list(&mut self, ui: &mut Ui, lib: &Library) {
        let favorites = self.favorites.get_or_insert_with(tracks::Favorites::load).clone();
        // Tabs.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let count = |f: &dyn Fn(&tracks::Track) -> bool| lib.tracks.iter().filter(|t| f(t)).count();
            for (view, label, n) in [
                (TrackView::All, "All", lib.tracks.len()),
                (TrackView::Favorites, "Favorites", count(&|t| favorites.contains(&t.guid))),
                (TrackView::Custom, "Custom", count(&|t| t.kind == Kind::Custom)),
                (TrackView::Raced, "Raced", count(&|t| !t.sessions.is_empty())),
            ] {
                if chip(ui, &format!("{label}  {}", group_thousands(n)), self.track_view == view).clicked() {
                    self.track_view = view;
                }
            }
        });
        ui.add_space(8.0);
        search_field(ui, &mut self.track_filter, "Search tracks, cities, regions");
        ui.add_space(8.0);

        let needle = self.track_filter.to_lowercase();
        let searching = !needle.is_empty();
        let in_view = |t: &tracks::Track| match self.track_view {
            TrackView::All => true,
            TrackView::Favorites => favorites.contains(&t.guid),
            TrackView::Custom => t.kind == Kind::Custom,
            TrackView::Raced => !t.sessions.is_empty(),
        };
        let matches = |t: &tracks::Track| in_view(t) && (!searching || t.search.contains(&needle));
        let by_name = |a: &usize, b: &usize| lib.tracks[*a].sort_key.cmp(&lib.tracks[*b].sort_key);

        // Rows: the All tab groups by country under collapsible headers; the
        // other tabs are short, flat lists.
        #[derive(Clone)]
        enum Row {
            Header(String, String, usize, bool),
            Track(usize),
        }
        const HEADER_H: f32 = 44.0;
        const TRACK_H: f32 = 76.0;
        let mut rows: Vec<Row> = Vec::new();
        let mut system_header = None;
        let system = crate::countries::system_country();
        if self.track_view == TrackView::All {
            // Open the system country first (MYCHRONIX_TRACKS_COLLAPSED=1 skips that, for screenshots).
            if !self.track_list_ready && std::env::var("MYCHRONIX_TRACKS_COLLAPSED").is_err() {
                if let Some(code) = &system {
                    self.track_groups_open.insert(code.clone());
                }
            }
            let mut by_country: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
            for (i, t) in lib.tracks.iter().enumerate() {
                if matches(t) {
                    by_country.entry(t.country.clone()).or_default().push(i);
                }
            }
            let mut groups: Vec<(String, String, Vec<usize>)> =
                by_country.into_iter().map(|(code, mut v)| { v.sort_by(by_name); let l = crate::countries::label(&code); (code, l, v) }).collect();
            groups.sort_by(|a, b| a.1.cmp(&b.1));
            for (code, label, members) in groups {
                let open = searching || self.track_groups_open.contains(&code);
                if system.as_deref() == Some(code.as_str()) {
                    system_header = Some(rows.len());
                }
                rows.push(Row::Header(code.clone(), label, members.len(), open));
                if open {
                    rows.extend(members.into_iter().map(Row::Track));
                }
            }
        } else {
            let mut v: Vec<usize> = (0..lib.tracks.len()).filter(|&i| matches(&lib.tracks[i])).collect();
            v.sort_by(by_name);
            rows.extend(v.into_iter().map(Row::Track));
        }

        // Variable-height rows, drawn only where visible.
        let heights: Vec<f32> = rows.iter().map(|r| if matches!(r, Row::Header(..)) { HEADER_H } else { TRACK_H }).collect();
        let mut tops = Vec::with_capacity(rows.len());
        let mut y = 0.0;
        for h in &heights {
            tops.push(y);
            y += h;
        }
        let total = y;

        let mut scroll = egui::ScrollArea::vertical().id_salt(("track_list", self.track_view as u8)).auto_shrink([false, false]);
        if !self.track_list_ready {
            if let Some(r) = system_header {
                scroll = scroll.vertical_scroll_offset(tops[r]);
            }
            self.track_list_ready = true;
        }

        let mut clicked = None;
        let mut toggled = None;
        let mut starred = None;
        scroll.show_viewport(ui, |ui, viewport| {
            let full = ui.max_rect();
            ui.set_height(total);
            let first = tops.partition_point(|&t| t + TRACK_H < viewport.min.y);
            for k in first..rows.len() {
                if tops[k] > viewport.max.y {
                    break;
                }
                let rect = Rect::from_min_size(pos2(full.left(), full.top() + tops[k]), vec2(full.width(), heights[k]));
                let response = ui.interact(rect, ui.id().with(("track_row", k)), Sense::click());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let p = ui.painter();
                match &rows[k] {
                    Row::Header(code, label, count, open) => {
                        if response.hovered() {
                            p.rect_filled(rect.shrink2(vec2(0.0, 3.0)), CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE_HOVER);
                        }
                        let chevron = if *open { Icon::ChevronDown } else { Icon::ChevronRight };
                        chevron.image(SECONDARY, 16.0).paint_at(ui, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(16.0)));
                        p.text(pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, label, semibold(14.5), TEXT);
                        p.text(pos2(rect.right() - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, group_thousands(*count), regular(12.5), DIM);
                        if response.clicked() && !searching {
                            toggled = Some(code.clone());
                        }
                    }
                    Row::Track(i) => {
                        let i = *i;
                        let t = &lib.tracks[i];
                        let fill = if self.track_sel == Some(i) { Some(SURFACE_ACTIVE) } else if response.hovered() { Some(SURFACE_HOVER) } else { None };
                        if let Some(fill) = fill {
                            p.rect_filled(rect.shrink2(vec2(0.0, 2.0)), CornerRadius::same(theme::RADIUS_SMALL + 2), fill);
                        }
                        let indent = if self.track_view == TrackView::All { 8.0 } else { 0.0 };
                        let thumb = Rect::from_min_size(pos2(rect.left() + 8.0 + indent, rect.top() + 8.0), vec2(64.0, TRACK_H - 16.0));
                        outline_thumbnail(p, thumb, &t.outline, if t.kind == Kind::Custom { SERIES[1] } else { SECONDARY });
                        let left = thumb.right() + 12.0;
                        let title = t.full_name.as_deref().unwrap_or(&t.name);
                        let name_w = (rect.right() - left - 120.0).max(60.0);
                        p.galley(pos2(left, rect.top() + 9.0), p.layout_job(single_line(title, medium(14.0), TEXT, name_w)), TEXT);
                        let place = if self.track_view == TrackView::All {
                            t.locality()
                        } else {
                            [t.locality(), crate::countries::label(&t.country)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
                        };
                        p.text(pos2(left, rect.top() + 39.0), egui::Align2::LEFT_CENTER, place, regular(12.5), SECONDARY);
                        p.text(pos2(left, rect.top() + 57.0), egui::Align2::LEFT_CENTER, t.description(), regular(12.0), DIM);
                        // Right: favorite star, then the name on the device; custom tag and sessions below.
                        let star = Rect::from_center_size(pos2(rect.right() - 20.0, rect.top() + 18.0), Vec2::splat(26.0));
                        let star_resp = ui.interact(star, ui.id().with(("track_star", k)), Sense::click());
                        let fav = favorites.contains(&t.guid);
                        let (icon, tint) = if fav { (Icon::StarFilled, theme::FAVORITE) } else if star_resp.hovered() { (Icon::Star, TEXT) } else { (Icon::Star, DIM) };
                        icon.image(tint, 16.0).paint_at(ui, Rect::from_center_size(star.center(), Vec2::splat(16.0)));
                        if star_resp.clicked() {
                            starred = Some(t.guid.clone());
                        }
                        let p = ui.painter();
                        p.text(pos2(star.left() - 4.0, rect.top() + 18.0), egui::Align2::RIGHT_CENTER, &t.name, regular(12.0), DIM);
                        let mut right = rect.right() - 12.0;
                        if !t.sessions.is_empty() {
                            let n = t.sessions.len();
                            let g = p.text(pos2(right, rect.top() + 50.0), egui::Align2::RIGHT_CENTER, plural(n, "session"), regular(12.5), SECONDARY);
                            right = g.left() - 8.0;
                        }
                        if t.kind == Kind::Custom {
                            let galley = p.layout_no_wrap("Custom".into(), semibold(11.0), TEXT);
                            let r = Rect::from_min_size(pos2(right - galley.size().x - 14.0, rect.top() + 40.0), galley.size() + vec2(14.0, 6.0));
                            p.rect_stroke(r, CornerRadius::same(255), Stroke::new(1.0, DIM), egui::StrokeKind::Inside);
                            p.galley(r.center() - galley.size() / 2.0, galley, TEXT);
                        }
                        if response.clicked() && !star_resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                }
            }
        });
        if rows.is_empty() {
            let msg = match self.track_view {
                TrackView::Favorites if !searching => "No favorites yet. Star a track to keep it here.",
                TrackView::Custom if !searching => "No custom tracks yet. They come over from your MyChron when you connect.",
                _ => "No tracks match.",
            };
            ui.label(RichText::new(msg).color(DIM));
        }
        if let Some(code) = toggled {
            if !self.track_groups_open.remove(&code) {
                self.track_groups_open.insert(code);
            }
        }
        if let Some(guid) = starred {
            if let Some(f) = self.favorites.as_mut() {
                if let Err(e) = f.toggle(&guid) {
                    self.error = Some(format!("Couldn't save favorites: {e}"));
                }
            }
        }
        if let Some(i) = clicked {
            self.select_track(lib, i);
        }
    }

    fn track_detail(&mut self, ui: &mut Ui, lib: &Library, i: usize, map_h: f32) {
        let t = &lib.tracks[i];
        let file = self.track_file.as_ref().filter(|(j, _)| *j == i).map(|(_, f)| f);
        let info = file.map(|f| &f.info);

        // AiM's details for the venue, laid out like RaceStudio's track view.
        section(ui, |ui| {
            ui.horizontal_top(|ui| {
                let logo_w = 150.0;
                let text_w = ui.available_width() - logo_w - 12.0;
                ui.allocate_ui_with_layout(vec2(text_w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let title = t.full_name.as_deref().unwrap_or(&t.name);
                    let fav = self.favorites.get_or_insert_with(tracks::Favorites::load).contains(&t.guid);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).font(semibold(22.0)).color(TEXT));
                        let (icon, label) = if fav { (Icon::StarFilled, "Favorite") } else { (Icon::Star, "Add to favorites") };
                        if button_with(ui, Some(icon), fav.then_some(theme::FAVORITE), label, false).clicked() {
                            if let Err(e) = self.favorites.as_mut().unwrap().toggle(&t.guid) {
                                self.error = Some(format!("Couldn't save favorites: {e}"));
                            }
                        }
                    });
                    let kind = if t.kind == Kind::Custom { "Custom track" } else { "AiM track" };
                    ui.label(RichText::new(format!("{kind}  ·  name on device: {}", t.name)).size(13.0).color(SECONDARY));
                    ui.add_space(6.0);
                    let address = [
                        info.and_then(|i| i.address.clone()),
                        info.and_then(|i| i.postcode.clone()),
                        info.and_then(|i| i.city.clone()).or(t.city.clone()),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("  ·  ");
                    if !address.is_empty() {
                        ui.label(RichText::new(address).color(TEXT));
                    }
                    let place = [t.region.clone(), crate::countries::label(&t.country)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
                    ui.label(RichText::new(place).color(TEXT));
                    if let Some(phone) = info.and_then(|i| i.phone.as_deref()) {
                        ui.label(RichText::new(phone).color(SECONDARY));
                    }
                    if let Some(url) = info.and_then(|i| i.website.as_deref()) {
                        ui.hyperlink_to(RichText::new(url).color(SERIES[0]), url);
                    }
                    ui.add_space(6.0);
                    let desc = t.description();
                    if !desc.is_empty() {
                        ui.label(RichText::new(desc).font(medium(14.0)).color(TEXT));
                    }
                    if let Some(tz) = file.and_then(|f| f.timezone.as_deref()) {
                        ui.label(RichText::new(format!("Time zone: {tz}")).size(13.0).color(SECONDARY));
                    }
                    if let Some(sf) = t.start_finish {
                        let ns = if sf.lat >= 0.0 { "N" } else { "S" };
                        let ew = if sf.lon >= 0.0 { "E" } else { "W" };
                        ui.label(
                            RichText::new(format!("Start/finish: {:.7}° {ns}, {:.7}° {ew}", sf.lat.abs(), sf.lon.abs()))
                                .size(13.0)
                                .color(SECONDARY),
                        );
                    }
                    if let Some(comment) = info.and_then(|i| i.comment.as_deref()) {
                        ui.label(RichText::new(comment).size(13.0).color(SECONDARY));
                    }
                    // Everything AiM stores for the track, verbatim.
                    egui::CollapsingHeader::new(RichText::new("All AiM fields").size(13.0).color(SECONDARY))
                        .id_salt(("aim_fields", i))
                        .show(ui, |ui| {
                            egui::Grid::new(("aim_fields_grid", i)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                                let mut row = |k: &str, v: String| {
                                    ui.label(RichText::new(k).size(12.5).color(DIM));
                                    ui.label(RichText::new(v).size(12.5).color(TEXT));
                                    ui.end_row();
                                };
                                row("AiM id", t.id.clone());
                                row("Track guid", t.guid.clone());
                                if let Some(l) = t.length_m {
                                    row("Length", format!("{l:.1} m ({:.3} mi)", l / 1609.344));
                                }
                                row("Type", t.track_kind.label());
                                if let Some(f) = file {
                                    row("Outline", format!("{} points", f.outline.len()));
                                }
                                for (k, v) in info.map(|i| i.fields.clone()).unwrap_or_default() {
                                    row(&k, v);
                                }
                            });
                        });
                });
                if let Some(png) = file.and_then(|f| f.logo_png.clone()) {
                    ui.add(
                        egui::Image::from_bytes(format!("bytes://track-logo-{}.png", t.id), png)
                            .max_size(vec2(logo_w, 110.0))
                            .corner_radius(CornerRadius::same(6)),
                    );
                }
            });
        });
        ui.add_space(10.0);

        // Map: OpenStreetMap with the track drawn on top.
        let file_outline = self.track_file.as_ref().filter(|(j, _)| *j == i).map(|(_, f)| f.outline.clone()).unwrap_or_default();
        let outline: Vec<aim_track::LatLon> = if !t.outline.is_empty() { t.outline.clone() } else { file_outline };
        let lap = self.speed_laps.get(&t.guid).and_then(Option::as_ref);

        // Speed / splits / outline switch, when there's a lap to show.
        if self.split_count < 2 {
            self.split_count = 3;
        }
        // Elevation comes with the track file's outline (every AiM track has it).
        let elevation: Vec<f32> = self
            .track_file
            .as_ref()
            .filter(|(j, f)| *j == i && f.elevation_m.len() == outline.len())
            .map(|(_, f)| f.elevation_m.clone())
            .unwrap_or_default();
        let mut modes = Vec::new();
        if lap.is_some() {
            modes.extend([(MapMode::Speed, "Your speed"), (MapMode::Splits, "Splits")]);
        }
        if !elevation.is_empty() {
            modes.push((MapMode::Elevation, "Elevation"));
        }
        modes.push((MapMode::Outline, "Outline"));
        if !modes.iter().any(|(m, _)| *m == self.track_map) {
            // e.g. Your speed on a track you haven't raced: fall back to what this track has.
            self.track_map = modes[0].0;
        }
        if modes.len() > 1 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (mode, label) in &modes {
                    if chip(ui, label, self.track_map == *mode).clicked() {
                        self.track_map = *mode;
                    }
                }
                if self.track_map == MapMode::Splits {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for n in (2..=6).rev() {
                            if chip(ui, &n.to_string(), self.split_count == n).clicked() {
                                self.split_count = n;
                            }
                        }
                        ui.label(RichText::new("Sectors").size(12.5).color(DIM));
                    });
                }
            });
            ui.add_space(8.0);
        }
        let mode = self.track_map;
        let show_speed = mode == MapMode::Speed;
        // Splits are placed along the track's own outline when it has one, else along your lap.
        let sectors = (mode == MapMode::Splits)
            .then(|| {
                let reference = if !outline.is_empty() { outline.clone() } else { lap.map(|l| l.points.clone()).unwrap_or_default() };
                splits::Sectors::even(&reference, t.start_finish, self.split_count)
            })
            .flatten();
        let overlay = TrackOverlay {
            outline: if mode == MapMode::Outline {
                if !outline.is_empty() { outline.clone() } else { lap.map(|l| l.points.clone()).unwrap_or_default() }
            } else {
                Vec::new()
            },
            outline_color: if t.kind == Kind::Custom { SERIES[1] } else { SERIES[0] },
            speed: show_speed.then(|| lap.cloned()).flatten(),
            sectors: sectors.as_ref().map(|s| (s.path.clone(), s.boundaries.clone())),
            elevation: (mode == MapMode::Elevation).then(|| (outline.clone(), elevation.clone())),
            start_finish: t.start_finish,
        };

        let options = || walkers::HttpOptions {
            cache: crate::paths::cache_dir().map(|d| d.join("tiles")),
            user_agent: Some(walkers::HeaderValue::from_static("Mychronix/0.1 (AiM MyChron data analysis; Linux desktop)")),
            ..Default::default()
        };
        if self.map_tiles.is_none() {
            self.map_tiles = Some(walkers::HttpTiles::with_options(walkers::sources::OpenStreetMap, options(), ui.ctx().clone()));
        }
        if self.map_satellite && self.sat_tiles.is_none() {
            self.sat_tiles = Some(walkers::HttpTiles::with_options(EsriImagery, options(), ui.ctx().clone()));
        }
        let center = t.start_finish.map(|p| walkers::lat_lon(p.lat, p.lon)).unwrap_or(walkers::lat_lon(0.0, 0.0));
        // Zoom to fit the drawn track once per selection.
        if std::mem::take(&mut self.fit_map) {
            let drawn: Vec<aim_track::LatLon> = match (&overlay.speed, &overlay.sectors, &overlay.elevation) {
                (Some(l), _, _) => l.points.clone(),
                (None, Some((path, _)), _) => path.clone(),
                (None, None, Some((path, _))) => path.clone(),
                _ => overlay.outline.clone(),
            };
            if let Some((c, zoom)) = fit_view(&drawn, vec2(ui.available_width(), map_h)) {
                self.map_memory.center_at(walkers::lat_lon(c.lat, c.lon));
                let _ = self.map_memory.set_zoom(zoom);
            }
        }
        let mut toggle = false;
        Frame::new().corner_radius(CornerRadius::same(theme::RADIUS)).fill(SURFACE).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(map_h);
            let tiles: &mut walkers::HttpTiles = if self.map_satellite { self.sat_tiles.as_mut().unwrap() } else { self.map_tiles.as_mut().unwrap() };
            let map = ui.add(walkers::Map::new(Some(tiles), &mut self.map_memory, center).with_plugin(overlay));
            // Map / Satellite switch in the map's top-right corner.
            let corner = Rect::from_min_size(map.rect.right_top() + vec2(-190.0, 10.0), vec2(180.0, 36.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(corner).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if chip(ui, "Satellite", self.map_satellite).clicked() && !self.map_satellite {
                    toggle = true;
                }
                if chip(ui, "Map", !self.map_satellite).clicked() && self.map_satellite {
                    toggle = true;
                }
            });
        });
        if toggle {
            self.map_satellite = !self.map_satellite;
        }
        let credit = if self.map_satellite { "Imagery © Esri, Maxar, Earthstar Geographics." } else { "Map data © OpenStreetMap contributors." };
        // Offline, maps viewed before come from the cache; others draw the track on a blank background.
        let credit = if self.aim_tracks_offline { format!("{credit}  Offline: maps you haven't viewed before appear once you're online.") } else { credit.to_string() };
        let credit = credit.as_str();

        ui.add_space(6.0);
        match (mode, lap) {
            (MapMode::Speed, Some(lap)) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:.0}", crate::units::speed(lap.min_kmh))).size(12.0).color(DIM));
                    let (bar, _) = ui.allocate_exact_size(vec2(160.0, 8.0), Sense::hover());
                    for k in 0..40 {
                        let r = Rect::from_min_size(pos2(bar.left() + bar.width() * k as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                        ui.painter().rect_filled(r, 0.0, speed_color(k as f32 / 39.0));
                    }
                    ui.label(RichText::new(format!("{:.0} {}", crate::units::speed(lap.max_kmh), crate::units::speed_unit())).size(12.0).color(DIM));
                    ui.add_space(14.0);
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 8.0, TEXT);
                    ui.painter().text(dot.center(), egui::Align2::CENTER_CENTER, "1", semibold(10.5), ON_TEXT);
                    ui.label(RichText::new("Corners, at their slowest point (numbered as in Analysis → Corners)").size(12.0).color(DIM));
                });
                ui.label(RichText::new(format!("{}  {credit}", lap.source)).size(11.5).color(DIM));
            }
            (MapMode::Splits, Some(lap)) => {
                if let Some(sec) = &sectors {
                    ui.horizontal(|ui| {
                        for k in 0..sec.count() {
                            let (sw, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
                            ui.painter().line_segment([sw.left_center(), sw.right_center()], Stroke::new(4.0, SERIES[k % SERIES.len()]));
                            ui.label(RichText::new(format!("S{}", k + 1)).size(12.5).color(SECONDARY));
                            ui.add_space(6.0);
                        }
                    });
                }
                ui.label(
                    RichText::new(format!("Splits placed by Mychronix at equal distances from the start/finish, timed from your GPS.  {credit}"))
                        .size(11.5)
                        .color(DIM),
                );
                ui.add_space(10.0);
                if let Some(sec) = &sectors {
                    split_table(ui, lap, sec);
                }
            }
            (MapMode::Elevation, _) => {
                let (lo, hi) = elevation.iter().fold((f32::MAX, f32::MIN), |(a, b), &e| (a.min(e), b.max(e)));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{lo:.0}")).size(12.0).color(DIM));
                    let (bar, _) = ui.allocate_exact_size(vec2(160.0, 8.0), Sense::hover());
                    for k in 0..40 {
                        let r = Rect::from_min_size(pos2(bar.left() + bar.width() * k as f32 / 40.0, bar.top()), vec2(bar.width() / 40.0 + 0.5, bar.height()));
                        ui.painter().rect_filled(r, 0.0, speed_color(k as f32 / 39.0));
                    }
                    ui.label(RichText::new(format!("{hi:.0} m above sea level  ·  {:.0} m between lowest and highest point", hi - lo)).size(12.0).color(DIM));
                });
                ui.label(RichText::new(format!("Elevation from AiM's track file, as in RaceStudio's track preview.  {credit}")).size(11.5).color(DIM));
            }
            _ => {
                let note = if outline.is_empty() {
                    "AiM tracks give the start/finish only; record a session here to see the shape."
                } else if t.kind == Kind::Custom {
                    "Outline from the custom track file."
                } else {
                    "Outline from AiM's track file."
                };
                ui.label(RichText::new(format!("{note}  {credit}")).size(11.5).color(DIM));
            }
        }
        ui.add_space(10.0);

        // Sessions recorded here.
        let mut open = None;
        section(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Your sessions here").font(semibold(16.0)).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(best) = t.best_lap_ms(lib) {
                        ui.label(RichText::new(format_lap_time(best as i64)).font(semibold(16.0)).color(theme::GOOD));
                    }
                });
            });
            ui.add_space(6.0);
            if t.sessions.is_empty() {
                ui.label(RichText::new("None yet. Sessions you download from your MyChron show up here.").color(SECONDARY));
                return;
            }
            let track_best = t.best_lap_ms(lib);
            // Part of the page (no scroll box of its own), so scrolling the page reaches every session.
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for &si in &t.sessions {
                    let s = &lib.sessions[si];
                    let sum = &s.summary;
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
                    if ui.is_rect_visible(rect) {
                        let p = ui.painter();
                        if response.hovered() {
                            p.rect_filled(rect, CornerRadius::same(theme::RADIUS_SMALL + 2), SURFACE_HOVER);
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let (l, r) = (rect.left() + 10.0, rect.right() - 10.0);
                        let (top, bottom) = (rect.top() + 15.0, rect.top() + 33.0);
                        p.text(pos2(l, top), egui::Align2::LEFT_CENTER, format!("Session {}", sum.number.unwrap_or(0)), medium(14.0), TEXT);
                        let when = [sum.date.map(crate::events::us_short), sum.start_time.as_deref().map(crate::events::clock_time)].into_iter().flatten().collect::<Vec<_>>().join("  ·  ");
                        p.text(pos2(l, bottom), egui::Align2::LEFT_CENTER, when, regular(12.5), DIM);
                        let is_best = sum.best_lap_ms.is_some() && sum.best_lap_ms == track_best;
                        let best = sum.best_lap_ms.map(|b| format_lap_time(b as i64)).unwrap_or_else(|| "—".into());
                        p.text(pos2(r, top), egui::Align2::RIGHT_CENTER, best, medium(14.0), if is_best { theme::GOOD } else { TEXT });
                        let laps = sum.laps.unwrap_or(0);
                        p.text(pos2(r, bottom), egui::Align2::RIGHT_CENTER, plural(laps as usize, "lap"), regular(12.5), DIM);
                    }
                    if response.clicked() {
                        open = Some(s.path.clone());
                    }
                }
            });
        });
        if let Some(path) = open {
            self.load(path);
        }
    }
}

/// One line of text cut to `width` with an ellipsis.
fn single_line(text: &str, font: egui::FontId, color: Color32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    job
}

/// A small drawing of a track's outline, fitted into `rect`.
fn outline_thumbnail(p: &egui::Painter, rect: Rect, outline: &[aim_track::LatLon], color: Color32) {
    p.rect_filled(rect, CornerRadius::same(6), WINDOW);
    if outline.len() < 2 {
        return;
    }
    let lat0 = outline[0].lat;
    let kx = lat0.to_radians().cos();
    let xy: Vec<(f64, f64)> = outline.iter().map(|q| (q.lon * kx, q.lat)).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for &(x, y) in &xy {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    let area = rect.shrink(6.0);
    let scale = (area.width() as f64 / (x1 - x0).max(1e-9)).min(area.height() as f64 / (y1 - y0).max(1e-9));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let pts: Vec<_> = xy
        .iter()
        .map(|&(x, y)| pos2(area.center().x + ((x - cx) * scale) as f32, area.center().y - ((y - cy) * scale) as f32))
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
}

/// Your fastest lap at a track: where you drove, how fast, and where you braked hardest.
#[derive(Clone)]
pub(super) struct SpeedLap {
    points: Vec<aim_track::LatLon>,
    kmh: Vec<f64>,
    min_kmh: f64,
    max_kmh: f64,
    brakes: Vec<Brake>,
    /// Which lap this is, for the caption.
    source: String,
    /// The whole session it came from, for timing splits on every lap.
    session_number: u32,
    session_laps: Vec<aim_xrk::Lap>,
    session_times: Vec<i64>,
    session_points: Vec<aim_track::LatLon>,
}

/// A corner on the speed map, numbered as on Analysis → Corners.
#[derive(Clone, Copy)]
struct Brake {
    /// Index into `points` of the corner's slowest point.
    at: usize,
    number: usize,
}

/// The best lap of your best session at `t`; if the logger timed no laps
/// there, one loop cut from your longest session.
fn speed_lap(lib: &Library, t: &tracks::Track) -> Option<SpeedLap> {
    let timed = t
        .sessions
        .iter()
        .filter(|&&i| lib.sessions[i].summary.best_lap_ms.is_some())
        .min_by_key(|&&i| lib.sessions[i].summary.best_lap_ms);
    let longest = t.sessions.iter().max_by_key(|&&i| lib.sessions[i].summary.duration_ms.unwrap_or(0));
    let pick = *timed.or(longest)?;
    let session = crate::jobs::open_session(&lib.sessions[pick].path).ok()?;
    let gps = session.gps.as_ref()?;
    let all: Vec<(i64, aim_track::LatLon, f64)> = (0..gps.len())
        .map(|i| (gps.times_ms[i], aim_track::LatLon { lat: gps.latitude[i], lon: gps.longitude[i] }, gps.speed_kmh[i]))
        .collect();
    let number = lib.sessions[pick].summary.number.unwrap_or(0);
    let (range, source) = match session.best_lap(MIN_LAP_MS) {
        Some(lap) => {
            let lo = all.partition_point(|p| p.0 < lap.start_ms);
            let hi = all.partition_point(|p| p.0 <= lap.end_ms);
            (lo..hi, format!("Your best lap here: session {number}, lap {} ({}).", lap.number, format_lap_time(lap.duration_ms())))
        }
        None => (one_loop(&all), format!("One lap from session {number} (no timed laps here).")),
    };
    let lap = &all[range];
    if lap.len() < 10 {
        return None;
    }
    let times: Vec<i64> = lap.iter().map(|p| p.0).collect();
    let kmh = smooth(&lap.iter().map(|p| p.2).collect::<Vec<_>>(), 3);
    let (min_kmh, max_kmh) = robust_range(kmh.iter().copied());
    Some(SpeedLap {
        points: lap.iter().map(|p| p.1).collect(),
        brakes: corner_points(&times, &lap.iter().map(|p| p.2).collect::<Vec<_>>()),
        kmh,
        min_kmh,
        max_kmh,
        source,
        session_number: number,
        session_laps: session.complete_laps().to_vec(),
        session_times: all.iter().map(|p| p.0).collect(),
        session_points: all.iter().map(|p| p.1).collect(),
    })
}

/// Centered moving average over `±half` samples.
fn smooth(v: &[f64], half: usize) -> Vec<f64> {
    (0..v.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(v.len()));
            v[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect()
}

/// Each corner's slowest point, found and numbered the same way as on
/// Analysis → Corners (braking from the highest speed to the lowest).
fn corner_points(times_ms: &[i64], kmh: &[f64]) -> Vec<Brake> {
    crate::braking::find_zones(times_ms, kmh, |_| 0.0)
        .iter()
        .enumerate()
        .map(|(n, z)| Brake { at: times_ms.partition_point(|&t| t < z.apex_ms).min(times_ms.len() - 1), number: n + 1 })
        .collect()
}

/// Sector times for every complete lap of the session, best per sector in
/// green, with the ideal lap (best sectors added up).
fn split_table(ui: &mut Ui, lap: &SpeedLap, sectors: &splits::Sectors) {
    let points = sectors.split_points();
    let rows: Vec<(i32, i64, Vec<i64>)> = lap
        .session_laps
        .iter()
        .filter_map(|l| {
            splits::sector_times(&lap.session_times, &lap.session_points, l.start_ms, l.end_ms, &points)
                .map(|t| (l.number, l.duration_ms(), t))
        })
        .collect();
    section(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Sector times  ·  session {}", lap.session_number)).font(semibold(16.0)).color(TEXT));
        });
        ui.add_space(6.0);
        if rows.is_empty() {
            ui.label(RichText::new("No complete laps in this session pass every split.").color(SECONDARY));
            return;
        }
        let n = sectors.count();
        let best: Vec<i64> = (0..n).map(|k| rows.iter().map(|r| r.2[k]).min().unwrap_or(0)).collect();
        let best_lap = rows.iter().map(|r| r.1).min().unwrap_or(0);
        let ideal: i64 = best.iter().sum();
        let secs = |ms: i64| format!("{:.3}", ms as f64 / 1000.0);
        egui::Grid::new("split_table").num_columns(n + 2).spacing([28.0, 6.0]).striped(false).show(ui, |ui| {
            ui.label(RichText::new("Lap").size(12.5).color(DIM));
            for k in 0..n {
                ui.horizontal(|ui| {
                    let (sw, _) = ui.allocate_exact_size(vec2(12.0, 10.0), Sense::hover());
                    ui.painter().line_segment([sw.left_center(), sw.right_center()], Stroke::new(3.0, SERIES[k % SERIES.len()]));
                    ui.label(RichText::new(format!("S{}", k + 1)).size(12.5).color(DIM));
                });
            }
            ui.label(RichText::new("Lap time").size(12.5).color(DIM));
            ui.end_row();
            for (number, total, times) in &rows {
                ui.label(RichText::new(number.to_string()).color(SECONDARY));
                for (k, t) in times.iter().enumerate() {
                    let color = if *t == best[k] { theme::GOOD } else { TEXT };
                    ui.label(RichText::new(secs(*t)).font(medium(14.0)).color(color));
                }
                let color = if *total == best_lap { theme::GOOD } else { TEXT };
                ui.label(RichText::new(format_lap_time(*total)).font(medium(14.0)).color(color));
                ui.end_row();
            }
            ui.label(RichText::new("Ideal").font(semibold(14.0)).color(TEXT));
            for t in &best {
                ui.label(RichText::new(secs(*t)).font(semibold(14.0)).color(theme::GOOD));
            }
            ui.label(RichText::new(format_lap_time(ideal)).font(semibold(14.0)).color(theme::GOOD));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("Ideal lap is {} faster than your best ({}).", secs(best_lap - ideal), format_lap_time(best_lap)))
                .size(12.5)
                .color(SECONDARY),
        );
    });
}

/// Esri World Imagery: satellite and aerial photos. Free for personal,
/// non-commercial use with attribution.
struct EsriImagery;

impl walkers::sources::TileSource for EsriImagery {
    fn tile_url(&self, id: walkers::TileId) -> String {
        format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}", id.zoom, id.y, id.x)
    }

    fn attribution(&self) -> walkers::sources::Attribution {
        walkers::sources::Attribution {
            text: "Esri, Maxar, Earthstar Geographics",
            url: "https://www.esri.com/",
            logo_light: None,
            logo_dark: None,
        }
    }
}

/// Center and web-map zoom that fit `points` into a `size` pixel view with a margin.
fn fit_view(points: &[aim_track::LatLon], size: Vec2) -> Option<(aim_track::LatLon, f64)> {
    if points.len() < 2 {
        return None;
    }
    let (mut lat0, mut lat1, mut lon0, mut lon1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in points {
        lat0 = lat0.min(p.lat);
        lat1 = lat1.max(p.lat);
        lon0 = lon0.min(p.lon);
        lon1 = lon1.max(p.lon);
    }
    let c = aim_track::LatLon { lat: (lat0 + lat1) / 2.0, lon: (lon0 + lon1) / 2.0 };
    let width_m = aim_track::LatLon { lat: c.lat, lon: lon0 }.distance_m(&aim_track::LatLon { lat: c.lat, lon: lon1 }).max(20.0);
    let height_m = aim_track::LatLon { lat: lat0, lon: c.lon }.distance_m(&aim_track::LatLon { lat: lat1, lon: c.lon }).max(20.0);
    // Meters per pixel at zoom z (256-px tiles): 156543.03 × cos(lat) / 2^z.
    let mpp = (width_m / (size.x as f64 * 0.75)).max(height_m / (size.y as f64 * 0.75));
    let zoom = (156_543.03 * c.lat.to_radians().cos() / mpp).log2().clamp(3.0, 19.5);
    Some((c, zoom))
}


/// The first closed loop in a GPS trace: from the first moment the kart is
/// moving to where it first comes back within 15 m after covering 250 m.
fn one_loop(points: &[(i64, aim_track::LatLon, f64)]) -> std::ops::Range<usize> {
    let Some(start) = points.iter().position(|(_, _, kmh)| *kmh > 20.0) else {
        return 0..points.len();
    };
    let origin = points[start].1;
    let mut traveled = 0.0;
    for j in start + 1..points.len() {
        traveled += points[j - 1].1.distance_m(&points[j].1);
        if traveled > 250.0 && points[j].1.distance_m(&origin) < 15.0 {
            return start..j + 1;
        }
    }
    start..points.len()
}

/// Draws a track on the map: your speed (with braking points) or its outline, and the start/finish.
struct TrackOverlay {
    outline: Vec<aim_track::LatLon>,
    outline_color: Color32,
    speed: Option<SpeedLap>,
    /// Splits view: the reference path and where each sector after the first begins.
    sectors: Option<(Vec<aim_track::LatLon>, Vec<usize>)>,
    /// Elevation view: the outline and each point's height in meters.
    elevation: Option<(Vec<aim_track::LatLon>, Vec<f32>)>,
    start_finish: Option<aim_track::LatLon>,
}

impl walkers::Plugin for TrackOverlay {
    fn run(self: Box<Self>, ui: &mut Ui, response: &egui::Response, projector: &walkers::Projector, _memory: &walkers::MapMemory) {
        // Draw only inside the map's visible area (it may be partly scrolled out of view).
        ui.set_clip_rect(response.rect.intersect(ui.clip_rect()));
        let painter = ui.painter();
        let to_screen = |p: &aim_track::LatLon| projector.project(walkers::lat_lon(p.lat, p.lon)).to_pos2();
        let casing = Stroke::new(8.0, Color32::from_black_alpha(120));
        if let Some(lap) = &self.speed {
            let pts: Vec<_> = lap.points.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing));
            let span = (lap.max_kmh - lap.min_kmh).max(1.0);
            for k in 0..pts.len() - 1 {
                let t = ((lap.kmh[k] - lap.min_kmh) / span) as f32;
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(5.0, speed_color(t)));
            }
            // Corner numbers at each corner's slowest point, as on Analysis → Corners.
            for b in &lap.brakes {
                let c = pts[b.at];
                painter.circle_filled(c, 12.0, Color32::from_black_alpha(170));
                painter.circle_filled(c, 10.0, TEXT);
                painter.text(c, egui::Align2::CENTER_CENTER, b.number.to_string(), semibold(12.0), ON_TEXT);
            }
        } else if let Some((path, heights)) = &self.elevation {
            let pts: Vec<_> = path.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing));
            let (lo, hi) = heights.iter().fold((f32::MAX, f32::MIN), |(a, b), &e| (a.min(e), b.max(e)));
            let span = (hi - lo).max(0.5);
            for k in 0..pts.len().saturating_sub(1) {
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(5.0, speed_color((heights[k] - lo) / span)));
            }
        } else if let Some((path, boundaries)) = &self.sectors {
            let pts: Vec<_> = path.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), casing));
            let mut sector = 0;
            for k in 0..pts.len() - 1 {
                while sector < boundaries.len() && k >= boundaries[sector] {
                    sector += 1;
                }
                painter.line_segment([pts[k], pts[k + 1]], Stroke::new(5.0, SERIES[sector % SERIES.len()]));
            }
            for (n, &b) in boundaries.iter().enumerate() {
                let c = pts[b];
                painter.circle_filled(c, 8.0, Color32::from_black_alpha(170));
                painter.circle_filled(c, 5.5, TEXT);
                let galley = painter.layout_no_wrap(format!("S{}", n + 2), semibold(11.5), TEXT);
                let r = Rect::from_min_size(c + vec2(10.0, -galley.size().y / 2.0 - 3.0), galley.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(190));
                painter.galley(r.min + vec2(5.0, 3.0), galley, TEXT);
            }
        } else if self.outline.len() > 1 {
            let pts: Vec<_> = self.outline.iter().map(to_screen).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(7.0, Color32::from_black_alpha(110))));
            painter.add(egui::Shape::line(pts, Stroke::new(4.0, self.outline_color)));
        }
        if let Some(sf) = &self.start_finish {
            let c = to_screen(sf);
            painter.circle_filled(c, 9.0, Color32::from_black_alpha(140));
            painter.circle_filled(c, 7.0, TEXT);
            Icon::Flag.image(ON_TEXT, 10.0).paint_at(ui, Rect::from_center_size(c, Vec2::splat(10.0)));
        }
    }
}
