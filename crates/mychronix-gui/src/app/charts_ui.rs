//! The Histogram and Scatter tabs (RaceStudio's Histogram and Scatter layouts).

use super::*;
use crate::charts;
use crate::report::short_name;

/// Histogram and scatter choices, kept while the app runs.
pub(super) struct ChartsState {
    pub hist_channel: String,
    /// Bars as % of time (else seconds).
    pub hist_percent: bool,
    pub hist_best: bool,
    pub x: String,
    pub y: String,
    pub color: Option<String>,
}

impl Default for ChartsState {
    fn default() -> Self {
        Self {
            hist_channel: "RPM".into(),
            hist_percent: true,
            hist_best: true,
            x: "GPS Speed".into(),
            y: "RPM".into(),
            color: None,
        }
    }
}

/// Channels worth a one-click pick, when the session has them.
const QUICK: [&str; 5] = ["RPM", "GPS Speed", "Exhaust Temp", "AccelerometerY", "GPS LonAcc"];

impl App {
    /// The time windows the charts cover: the chosen lap, or every racing lap.
    fn chart_windows(&self, session: &Session) -> (Vec<(i64, i64)>, String) {
        match self.lap.and_then(|i| session.laps.get(i)) {
            Some(l) => (vec![(l.start_ms, l.end_ms)], format!("Lap {}", l.number)),
            None => {
                let slow = slow_laps(session);
                let w: Vec<(i64, i64)> = session.laps.iter().zip(&slow).filter(|(_, s)| !**s).map(|(l, _)| (l.start_ms, l.end_ms)).collect();
                let label = format!("{} racing laps", w.len());
                (w, label)
            }
        }
    }

    /// A row of quick channel picks plus a dropdown of every live channel.
    fn channel_choice(ui: &mut Ui, id: &str, session: &Session, value: &mut String, quick: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if quick {
                for name in QUICK.iter().filter(|n| session.channels.iter().any(|c| c.name == **n)) {
                    if chip(ui, short_name(name), value == name).clicked() {
                        *value = name.to_string();
                    }
                }
            }
            egui::ComboBox::from_id_salt(id)
                .selected_text(if quick && QUICK.contains(&value.as_str()) { "Other…".to_string() } else { short_name(value).to_string() })
                .width(180.0)
                .show_ui(ui, |ui| {
                    for c in session.channels.iter().filter(|c| !is_dead(c)) {
                        ui.selectable_value(value, c.name.clone(), short_name(&c.name));
                    }
                });
        });
    }

    pub(super) fn histogram_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        self.lap_strip(ui, session, false);
        ui.add_space(12.0);
        let (windows, scope) = self.chart_windows(session);
        let st = &mut self.charts;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Channel").size(12.5).color(DIM));
            Self::channel_choice(ui, "hist_channel", session, &mut st.hist_channel, true);
            ui.add_space(12.0);
            ui.label(RichText::new("Bars").size(12.5).color(DIM));
            if chip(ui, "% of time", st.hist_percent).clicked() {
                st.hist_percent = true;
            }
            if chip(ui, "Seconds", !st.hist_percent).clicked() {
                st.hist_percent = false;
            }
            ui.add_space(12.0);
            if chip(ui, "Overlay best lap", st.hist_best).on_hover_text("Outline the session's best lap for comparison").clicked() {
                st.hist_best = !st.hist_best;
            }
        });
        ui.add_space(12.0);
        let Some(c) = session.channels.iter().find(|c| c.name == st.hist_channel) else {
            card(ui, |ui| {
                ui.label(RichText::new("This session doesn't have that channel.").color(SECONDARY));
            });
            return;
        };
        let best = session.best_lap(MIN_LAP_MS).copied();
        let best_window = best.map(|b| vec![(b.start_ms, b.end_ms)]);
        // Bins over the range of this lap's (and the best lap's) values.
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for &(s, e) in windows.iter().chain(best_window.iter().flatten()) {
            let a = c.times_ms.partition_point(|&t| t < s);
            let b = c.times_ms.partition_point(|&t| t <= e);
            for &v in &c.values[a..b] {
                if v.is_finite() {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
        }
        if lo > hi {
            card(ui, |ui| {
                ui.label(RichText::new("No samples in this lap.").color(SECONDARY));
            });
            return;
        }
        let bin = charts::nice_bin(hi - lo, 30.0);
        let start = (lo / bin).floor() * bin;
        let bins = (((hi - start) / bin).floor() as usize + 1).max(1);
        let to_bars = |h: Vec<f64>| -> Vec<f64> {
            let total: f64 = h.iter().sum::<f64>().max(1e-9);
            if st.hist_percent { h.iter().map(|s| s / total * 100.0).collect() } else { h }
        };
        let main = to_bars(charts::histogram(c, &windows, start, bin, bins));
        let overlay = (st.hist_best && self.lap.and_then(|i| session.laps.get(i)).copied() != best)
            .then(|| best_window.as_ref().map(|w| to_bars(charts::histogram(c, w, start, bin, bins))))
            .flatten();
        let units = pretty_units(&c.units).to_string();
        // The most common band and the time near the top of the range.
        let top = main.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map(|(k, _)| start + bin * (k as f64 + 0.5));
        let decimals = if bin < 1.0 { 2 } else { 0 };
        graph_card(ui, (ui.available_height() - 4.0).max(260.0), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} · {scope}", short_name(&c.name))).font(medium(14.5)).color(TEXT));
                ui.label(RichText::new(format!("{} by {units}, bands of {bin} {units}", if st.hist_percent { "% of time" } else { "seconds" })).size(12.5).color(DIM));
                if let Some(t) = top {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{t:.decimals$} {units}")).font(semibold(14.0)).color(TEXT));
                        ui.label(RichText::new("Most time around").size(12.5).color(DIM));
                    });
                }
            });
            let bars: Vec<egui_plot::Bar> = main.iter().enumerate().map(|(k, &v)| egui_plot::Bar::new(start + bin * (k as f64 + 0.5), v).width(bin * 0.86)).collect();
            if super::is_calculated(&c.name) {
                super::calculated_note(ui);
            }
            Plot::new(("histogram", c.name.clone()))
                .height(ui.available_height())
                .show_background(false)
                .grid_color(GRID)
                .y_axis_min_width(46.0)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .show_x(false)
                .show_y(false)
                .include_y(0.0)
                .show(ui, |plot| {
                    plot.bar_chart(egui_plot::BarChart::new(scope.clone(), bars).color(SERIES[0]));
                    if let (Some(o), Some(b)) = (&overlay, best) {
                        let pts: Vec<[f64; 2]> = o
                            .iter()
                            .enumerate()
                            .flat_map(|(k, &v)| [[start + bin * k as f64, v], [start + bin * (k as f64 + 1.0), v]])
                            .collect();
                        plot.line(Line::new(format!("Best lap {}", b.number), PlotPoints::from(pts)).color(theme::GOOD).width(2.0));
                    }
                });
        });
    }

    pub(super) fn scatter_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        self.lap_strip(ui, session, false);
        ui.add_space(12.0);
        let (windows, scope) = self.chart_windows(session);
        let setup = self.current_setup();
        let st = &mut self.charts;
        let has = |n: &str| session.channels.iter().any(|c| c.name == n && !is_dead(c));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(RichText::new("Presets").size(12.5).color(DIM));
            if has("RPM") && has("GPS Speed") && chip(ui, "RPM vs speed", st.x == "GPS Speed" && st.y == "RPM").on_hover_text("The gearing as a line; points off it are clutch slip").clicked() {
                st.x = "GPS Speed".into();
                st.y = "RPM".into();
                st.color = None;
            }
            if has("AccelerometerY") && has("GPS LonAcc") && chip(ui, "G-G diagram", st.x == "AccelerometerY" && st.y == "GPS LonAcc").on_hover_text("Cornering against braking and acceleration").clicked() {
                st.x = "AccelerometerY".into();
                st.y = "GPS LonAcc".into();
                st.color = has("GPS Speed").then(|| "GPS Speed".into());
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(RichText::new("Across").size(12.5).color(DIM));
            Self::channel_choice(ui, "scatter_x", session, &mut st.x, false);
            ui.add_space(8.0);
            ui.label(RichText::new("Up").size(12.5).color(DIM));
            Self::channel_choice(ui, "scatter_y", session, &mut st.y, false);
            ui.add_space(8.0);
            ui.label(RichText::new("Color by").size(12.5).color(DIM));
            let mut color = st.color.clone().unwrap_or_default();
            egui::ComboBox::from_id_salt("scatter_color")
                .selected_text(if color.is_empty() { "Nothing".to_string() } else { short_name(&color).to_string() })
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut color, String::new(), "Nothing");
                    for c in session.channels.iter().filter(|c| !is_dead(c)) {
                        ui.selectable_value(&mut color, c.name.clone(), short_name(&c.name));
                    }
                });
            st.color = (!color.is_empty()).then_some(color);
        });
        ui.add_space(12.0);
        let find = |n: &str| session.channels.iter().find(|c| c.name == n);
        let (Some(x), Some(y)) = (find(&st.x), find(&st.y)) else {
            card(ui, |ui| {
                ui.label(RichText::new("Pick two channels this session has.").color(SECONDARY));
            });
            return;
        };
        let color = st.color.as_deref().and_then(find);
        let pts = charts::scatter(x, y, color, &windows, 6000);
        let gg = st.x == "AccelerometerY" && st.y == "GPS LonAcc";
        // The gearing on RPM vs speed: rpm = (rpm per unit of speed) × speed.
        let gearing = (st.x == "GPS Speed" && st.y == "RPM")
            .then(|| setup.as_ref().and_then(|s| s.expected_rpm_per_kmh().map(|e| (e / crate::units::speed(1.0), format!("{}/{}", s.drive_teeth.trim(), s.driven_teeth.trim())))))
            .flatten();
        let (cmin, cmax) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[2]), b.max(p[2])));
        let (xu, yu) = (pretty_units(&x.units).to_string(), pretty_units(&y.units).to_string());
        graph_card(ui, (ui.available_height() - 4.0).max(300.0), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} against {} · {scope}", short_name(&y.name), short_name(&x.name))).font(medium(14.5)).color(TEXT));
                ui.label(RichText::new(format!("{yu} up, {xu} across")).size(12.5).color(DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let (Some(c), true) = (color, cmax > cmin) {
                        let unit = pretty_units(&c.units);
                        ui.label(RichText::new(format!("{cmax:.0} {unit}")).size(12.0).color(DIM));
                        let (bar, _) = ui.allocate_exact_size(vec2(120.0, 8.0), Sense::hover());
                        for j in 0..30 {
                            let r = Rect::from_min_size(pos2(bar.left() + bar.width() * j as f32 / 30.0, bar.top()), vec2(bar.width() / 30.0 + 0.5, bar.height()));
                            ui.painter().rect_filled(r, 0.0, speed_color(j as f32 / 29.0));
                        }
                        ui.label(RichText::new(format!("{} {cmin:.0}", short_name(&c.name))).size(12.0).color(DIM));
                    } else if let Some((_, g)) = &gearing {
                        ui.label(RichText::new(format!("Line: {g} gearing from the setup sheet")).size(12.0).color(DIM));
                    }
                });
            });
            if super::is_calculated(&st.x) || super::is_calculated(&st.y) {
                super::calculated_note(ui);
            }
            let mut plot = Plot::new(("scatter", st.x.clone(), st.y.clone()))
                .height(ui.available_height())
                .show_background(false)
                .grid_color(GRID)
                .y_axis_min_width(46.0)
                .allow_scroll(false)
                .show_x(false)
                .show_y(false);
            if gg {
                plot = plot.data_aspect(1.0);
            }
            plot.show(ui, |plot| {
                if gg {
                    // Rings at 0.5, 1 and 1.5 g, as RaceStudio's circular grid.
                    for r in [0.5, 1.0, 1.5] {
                        let ring: Vec<[f64; 2]> = (0..=72).map(|k| {
                            let a = std::f64::consts::TAU * k as f64 / 72.0;
                            [r * a.cos(), r * a.sin()]
                        }).collect();
                        plot.line(Line::new(format!("{r} g"), PlotPoints::from(ring)).color(GRID).width(1.0));
                    }
                }
                match color {
                    Some(_) if cmax > cmin => {
                        // A dozen color bands, each its own set of points.
                        const BANDS: usize = 12;
                        let mut groups: Vec<Vec<[f64; 2]>> = vec![Vec::new(); BANDS];
                        for p in &pts {
                            let k = (((p[2] - cmin) / (cmax - cmin)) * (BANDS - 1) as f64).round() as usize;
                            groups[k.min(BANDS - 1)].push([p[0], p[1]]);
                        }
                        for (k, g) in groups.into_iter().enumerate().filter(|(_, g)| !g.is_empty()) {
                            plot.points(egui_plot::Points::new(format!("band {k}"), PlotPoints::from(g)).color(speed_color(k as f32 / (BANDS - 1) as f32)).radius(1.8));
                        }
                    }
                    _ => {
                        let xy: Vec<[f64; 2]> = pts.iter().map(|p| [p[0], p[1]]).collect();
                        plot.points(egui_plot::Points::new("samples", PlotPoints::from(xy)).color(SERIES[0].gamma_multiply(0.7)).radius(1.8));
                    }
                }
                if let Some((per, label)) = &gearing {
                    // Across the data only, so the plot isn't stretched down to 0.
                    let (lo, hi) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
                    if lo < hi {
                        plot.line(Line::new(format!("{label} gearing"), PlotPoints::from(vec![[lo, per * lo], [hi, per * hi]])).color(theme::GOOD).width(2.0));
                    }
                }
            });
        });
    }
}
