//! The Report tab: RaceStudio's Channels Report for the open session.

use super::*;
use crate::report::{self, Column, Preset, ReportConfig, Stat};

/// Report state for the open session.
#[derive(Default)]
pub(super) struct ReportState {
    pub config: Option<ReportConfig>,
    /// Laps left out of the statistics, by lap number, for `for_path`.
    pub excluded: std::collections::BTreeSet<i32>,
    pub for_path: Option<PathBuf>,
    /// Column plotted against lap number (0 = lap time).
    pub plotted: usize,
    pub hovered_lap: Option<i32>,
    /// The "add a column" picker.
    pub adding: Option<(String, Stat)>,
}

/// The least height the laps table gets under the chart: its summary rows and a few laps.
const LAPS_TABLE_MIN_H: f32 = 300.0;

/// What the Laps page's chart and table show, worked out once at the top.
struct LapsShown<'a> {
    session: &'a Session,
    columns: &'a [Column],
    /// Values: [lap][column], lap time first.
    cells: Vec<Vec<Option<f64>>>,
    n_cols: usize,
    /// The laps in the statistics (ticked), by index.
    included: Vec<usize>,
    summaries: Vec<Option<[f64; 4]>>,
    decimals: Vec<usize>,
    units: Vec<String>,
    best: Option<aim_xrk::Lap>,
    /// Where each lap lost the most against the best of each corner (laps that count).
    lost: Vec<Option<(usize, usize, f64)>>,
    /// 1 when there's a "Lost most at" column, 0 without.
    lost_col: usize,
    excluded: &'a std::collections::BTreeSet<i32>,
    plotted: usize,
    hovered_lap: Option<i32>,
}

impl<'a> LapsShown<'a> {
    fn new(
        session: &'a Session,
        columns: &'a [Column],
        excluded: &'a std::collections::BTreeSet<i32>,
        (plotted, hovered_lap): (usize, Option<i32>),
        model: Option<&crate::corner_model::CornerModel>,
    ) -> Self {
        let laps = &session.laps;
        let cells = report::table(session, columns);
        let n_cols = columns.len() + 1;
        let included: Vec<usize> = (0..laps.len())
            .filter(|&i| !excluded.contains(&laps[i].number))
            .collect();
        let summaries = report::summaries(&cells, n_cols, &included);
        let units: Vec<String> = std::iter::once("s".to_string())
            .chain(columns.iter().map(|c| {
                if c.stat.is_share() {
                    "% of lap".to_string()
                } else if c.stat == Stat::Corrections {
                    "per lap".to_string()
                } else {
                    report::channel(session, &c.channel)
                        .map(|ch| Shown::of(ch).unit().to_string())
                        .unwrap_or_default()
                }
            }))
            .collect();
        let lost: Vec<Option<(usize, usize, f64)>> = (0..laps.len())
            .map(|i| {
                let m = model?;
                let t = m.trace_of(i).filter(|&t| m.counts[t])?;
                let k = m.worst(t);
                Some((t, k, m.loss(t, k)))
            })
            .collect();
        let lost_col = usize::from(lost.iter().any(Option::is_some));
        Self {
            session,
            columns,
            cells,
            n_cols,
            included,
            summaries,
            decimals: report::decimals(session, columns),
            units,
            best: session.best_lap(MIN_LAP_MS).copied(),
            lost,
            lost_col,
            excluded,
            plotted,
            hovered_lap,
        }
    }

    /// How column `k` is written.
    fn fmt(&self, k: usize, v: f64) -> String {
        match k {
            0 => format_lap_time((v * 1000.0).round() as i64),
            _ if self.columns[k - 1].stat.is_share() => format!("{v:.0}%"),
            _ => format!("{v:.*}", self.decimals[k]),
        }
    }

    /// Column `k`'s title.
    fn title(&self, k: usize) -> String {
        if k == 0 {
            "Lap time".to_string()
        } else {
            self.columns[k - 1].title()
        }
    }
}

/// What was clicked or pointed at on the Laps page, applied once it's drawn.
#[derive(Default)]
struct LapsInput {
    /// A column to plot.
    plot: Option<usize>,
    /// A column to remove.
    remove: Option<usize>,
    /// A lap ticked or unticked, by number.
    toggle: Option<i32>,
    /// The lap under the pointer, by number.
    hovered: Option<i32>,
    /// A lap's corner to see in Corners: (lap index, trace, corner).
    to_corner: Option<(usize, usize, usize)>,
}

impl ReportState {
    /// A new session: out/in and slow laps start excluded, as RaceStudio leaves them out.
    fn open(&mut self, path: &Option<PathBuf>, session: &Session) {
        if self.for_path == *path {
            return;
        }
        self.for_path = path.clone();
        let slow = slow_laps(session);
        self.excluded = session
            .laps
            .iter()
            .zip(&slow)
            .filter(|(_, s)| **s)
            .map(|(l, _)| l.number)
            .collect();
        self.plotted = 0;
    }
}

impl App {
    pub(super) fn report_view(&mut self, ui: &mut Ui, session: &std::sync::Arc<Session>) {
        let model = self.corner_model(session, ui.ctx());
        self.laps_figures(ui, session, model.as_deref());
        ui.add_space(14.0);
        let st = &mut self.report;
        st.open(&self.path, session);
        let config = st.config.get_or_insert_with(ReportConfig::load);
        if config.columns.is_empty() {
            config.columns = Preset::Default.columns(session);
        }
        let columns = report::shown_columns(session, &config.columns);
        let mut changed = report_toolbar(ui, session, config, &mut st.adding);
        if let Some(adding) = &mut st.adding {
            ui.add_space(8.0);
            changed |= add_column(ui, session, config, adding);
        }
        ui.add_space(12.0);

        let shown = LapsShown::new(
            session,
            &columns,
            &st.excluded,
            (st.plotted, st.hovered_lap),
            model.as_deref(),
        );
        // A column filled by speed worked out from RPM says so.
        if columns.iter().any(|c| {
            report::channel(session, &c.channel).is_some_and(|ch| super::is_calculated(&ch.name))
        }) {
            super::calculated_note(ui);
            ui.add_space(6.0);
        }
        // The chart gives up height in a short window, so the table keeps room for laps.
        let chart_h = (ui.available_height() - 12.0 - LAPS_TABLE_MIN_H).clamp(120.0, 200.0);
        laps_chart(ui, &shown, chart_h);
        ui.add_space(12.0);
        // The table gets the rest of the height.
        let table_h = (ui.available_height() - 70.0).max(220.0);
        let mut input = LapsInput::default();
        laps_table(ui, &shown, table_h, &mut input);

        if let Some(k) = input.plot {
            st.plotted = k;
        }
        st.hovered_lap = input.hovered;
        if let Some(n) = input.toggle
            && !st.excluded.remove(&n)
        {
            st.excluded.insert(n);
        }
        if let Some(i) = input.remove {
            let col = columns[i].clone();
            config.columns.retain(|c| *c != col);
            st.plotted = 0;
            changed = true;
        }
        if changed && let Err(e) = config.save() {
            self.notices
                .error(format!("Couldn't save the report columns: {e}"));
        }
        if let Some((i, t, k)) = input.to_corner {
            self.analysis.lap = Some(i);
            self.analysis.cursor_s = None;
            self.corners.selected = Some((t, k));
            self.analysis.analysis_view = AnalysisView::Corners;
        }
    }
}

/// The presets and "+ Column" across the top; true when the columns changed.
fn report_toolbar(
    ui: &mut Ui,
    session: &Session,
    config: &mut ReportConfig,
    adding: &mut Option<(String, Stat)>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new("Presets").size(12.5).color(DIM));
        for p in Preset::ALL {
            if chip(ui, p.label(), false).clicked() {
                config.columns = p.columns(session);
                changed = true;
            }
        }
        ui.add_space(10.0);
        if chip(ui, "+ Column", adding.is_some()).clicked() {
            *adding = if adding.is_some() {
                None
            } else {
                Some((String::new(), Stat::Max))
            };
        }
    });
    changed
}

/// The "add a column" picker: a channel, a statistic, and Add; true when one was added.
fn add_column(
    ui: &mut Ui,
    session: &Session,
    config: &mut ReportConfig,
    (channel, stat): &mut (String, Stat),
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::ComboBox::from_id_salt("report_channel")
            .selected_text(if channel.is_empty() {
                "Channel…".to_string()
            } else {
                report::short_name(channel).to_string()
            })
            .width(220.0)
            .show_ui(ui, |ui| {
                for c in session
                    .channels
                    .iter()
                    .filter(|c| !is_dead(c) && !crate::channels::is_lap_timing(&c.name))
                {
                    let label = if report::short_name(&c.name) != c.name {
                        format!("{} ({})", report::short_name(&c.name), c.name)
                    } else {
                        c.name.clone()
                    };
                    ui.selectable_value(channel, c.name.clone(), label);
                }
            });
        for s in Stat::ALL {
            if chip(ui, s.label(), *stat == s).clicked() {
                *stat = s;
            }
        }
        if !channel.is_empty() && button(ui, Some(Icon::Check), "Add", true).clicked() {
            let col = Column {
                channel: channel.clone(),
                stat: *stat,
            };
            if !config.columns.contains(&col) {
                config.columns.push(col);
                changed = true;
            }
        }
    });
    changed
}

/// The plotted column against lap number, the lap under the pointer ringed.
fn laps_chart(ui: &mut Ui, shown: &LapsShown, height: f32) {
    let laps = &shown.session.laps;
    let k = shown.plotted.min(shown.n_cols - 1);
    let title = shown.title(k);
    let cells = &shown.cells;
    let pts: Vec<[f64; 2]> = shown
        .included
        .iter()
        .filter_map(|&i| cells[i][k].map(|v| [laps[i].number as f64, v]))
        .collect();
    let hot: Vec<[f64; 2]> = shown
        .hovered_lap
        .and_then(|n| laps.iter().position(|l| l.number == n))
        .and_then(|i| cells[i][k].map(|v| vec![[laps[i].number as f64, v]]))
        .unwrap_or_default();
    graph_card(ui, height, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{title} by lap"))
                    .font(medium(14.5))
                    .color(TEXT),
            );
            ui.label(RichText::new(&shown.units[k]).size(13.0).color(DIM));
        });
        Plot::new(("report_plot", k))
            .height(ui.available_height())
            // Steps of 1, 2, 4… rather than only 1, 10…, so a short chart still gets labels.
            .y_grid_spacer(egui_plot::log_grid_spacer(2))
            .show_background(false)
            .grid_color(GRID)
            .y_axis_min_width(46.0)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .show_x(false)
            .show_y(false)
            .x_axis_formatter(|m, _| {
                if (m.value - m.value.round()).abs() < 1e-6 {
                    format!("Lap {}", m.value.round())
                } else {
                    String::new()
                }
            })
            .show(ui, |plot| {
                plot.line(
                    Line::new(title.clone(), PlotPoints::from(pts.clone()))
                        .color(SERIES[0])
                        .width(2.0),
                );
                plot.points(
                    egui_plot::Points::new("laps", PlotPoints::from(pts))
                        .color(SERIES[0])
                        .radius(4.0),
                );
                if !hot.is_empty() {
                    plot.points(
                        egui_plot::Points::new("hovered", PlotPoints::from(hot))
                            .color(TEXT)
                            .radius(7.0),
                    );
                }
            });
    });
}

/// The laps table in its card: the statistics, then a row per lap, then what
/// the statistics are over.
fn laps_table(ui: &mut Ui, shown: &LapsShown, height: f32, input: &mut LapsInput) {
    let (n_cols, lost_col) = (shown.n_cols, shown.lost_col);
    card(ui, |ui| {
        // Headers stay put while the laps scroll; wide tables scroll sideways as a whole.
        egui::ScrollArea::horizontal()
            .id_salt("report_table_x")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .id_salt("report_table")
                    .striped(false)
                    .resizable(false)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(egui_extras::Column::auto().at_least(56.0))
                    .columns(egui_extras::Column::auto().at_least(64.0), n_cols)
                    .columns(egui_extras::Column::auto().at_least(120.0), lost_col)
                    // Fills the rest of the card, so the scrollbar sits at its edge.
                    .column(egui_extras::Column::remainder())
                    .min_scrolled_height(0.0)
                    .max_scroll_height(height)
                    .header(26.0, |mut header| laps_header(&mut header, shown, input))
                    .body(|mut body| {
                        summary_rows(&mut body, shown);
                        for i in 0..shown.session.laps.len() {
                            body.row(26.0, |mut row| lap_row(&mut row, shown, i, input));
                        }
                    });
            });
        ui.label(
            RichText::new(format!(
                "Max, min, average and std dev are over the {} ticked laps of {}. Out, in and slow laps start unticked.",
                shown.included.len(),
                shown.session.laps.len()
            ))
            .size(12.0)
            .color(DIM),
        );
    });
}

/// The table's titles: click one to plot it; × to remove.
fn laps_header(header: &mut egui_extras::TableRow, shown: &LapsShown, input: &mut LapsInput) {
    header.col(|ui| {
        ui.label(RichText::new("Lap").font(medium(13.0)).color(DIM));
    });
    for k in 0..shown.n_cols {
        header.col(|ui| {
            let title = shown.title(k);
            let color = if input.plot.unwrap_or(shown.plotted) == k {
                TEXT
            } else {
                SECONDARY
            };
            let r = ui.add(
                egui::Label::new(RichText::new(&title).font(medium(13.0)).color(color))
                    .sense(Sense::click()),
            );
            if r.on_hover_text(format!("Plot {title} by lap ({})", shown.units[k]))
                .clicked()
            {
                input.plot = Some(k);
            }
            if k > 0
                && ui
                    .add(Icon::X.image(DIM, 10.0).sense(Sense::click()))
                    .on_hover_text("Remove column")
                    .clicked()
            {
                input.remove = Some(k - 1);
            }
            ui.add_space(14.0);
        });
    }
    if shown.lost_col > 0 {
        header.col(|ui| {
            ui.label(RichText::new("Lost most at").font(medium(13.0)).color(SECONDARY))
                .on_hover_text("The corner where each lap lost the most against the best you drove it this session. Click to see it in Corners.");
        });
    }
    header.col(|_| {});
}

/// Statistics over the included laps, above the laps.
fn summary_rows(body: &mut egui_extras::TableBody, shown: &LapsShown) {
    for (j, name) in ["Max", "Min", "Average", "Std dev"].iter().enumerate() {
        body.row(24.0, |mut row| {
            row.col(|ui| {
                ui.label(RichText::new(*name).font(medium(12.5)).color(DIM));
            });
            for k in 0..shown.n_cols {
                row.col(|ui| {
                    let text = shown.summaries[k]
                        .map(|s| {
                            if j == 3 && k == 0 {
                                format!("{:.3}", s[3])
                            } else if j == 3 {
                                format!("{:.*}", shown.decimals[k] + 1, s[3])
                            } else {
                                shown.fmt(k, s[j])
                            }
                        })
                        .unwrap_or_else(|| "—".into());
                    ui.label(RichText::new(text).size(12.5).color(SECONDARY));
                });
            }
            if shown.lost_col > 0 {
                row.col(|_| {});
            }
            row.col(|_| {});
        });
    }
}

/// Lap `i`'s row: its tick, number, values, and the corner it lost most at.
fn lap_row(row: &mut egui_extras::TableRow, shown: &LapsShown, i: usize, input: &mut LapsInput) {
    let lap = shown.session.laps[i];
    let on = !shown.excluded.contains(&lap.number);
    let is_best = Some(lap) == shown.best;
    row.col(|ui| {
        let (cb, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
        ui.painter().rect_stroke(
            cb,
            CornerRadius::same(4),
            Stroke::new(1.3, if on { TEXT } else { DIM }),
            egui::StrokeKind::Inside,
        );
        if on {
            Icon::Check.image(TEXT, 12.0).paint_at(ui, cb.shrink(2.0));
        }
        if resp
            .on_hover_text(if on {
                "Leave out of the statistics"
            } else {
                "Include in the statistics"
            })
            .clicked()
        {
            input.toggle = Some(lap.number);
        }
        if ui
            .label(
                RichText::new(lap.number.to_string())
                    .font(medium(13.5))
                    .color(if on { TEXT } else { DIM }),
            )
            .hovered()
        {
            input.hovered = Some(lap.number);
        }
    });
    for k in 0..shown.n_cols {
        row.col(|ui| {
            let text = shown.cells[i][k]
                .map(|v| shown.fmt(k, v))
                .unwrap_or_else(|| "—".into());
            let color = if !on {
                DIM
            } else if k == 0 && is_best {
                theme::GOOD
            } else {
                TEXT
            };
            let font = if shown.hovered_lap == Some(lap.number) {
                semibold(13.5)
            } else {
                regular(13.5)
            };
            if ui
                .label(RichText::new(text).font(font).color(color))
                .hovered()
            {
                input.hovered = Some(lap.number);
            }
        });
    }
    if shown.lost_col > 0 {
        row.col(|ui| {
            let Some((t, k, loss)) = shown.lost[i] else {
                return;
            };
            ui.spacing_mut().item_spacing.x = 6.0;
            let r = ui.add(
                egui::Label::new(
                    RichText::new(format!("Corner {}", k + 1))
                        .size(13.5)
                        .color(if on { TEXT } else { DIM })
                        .underline(),
                )
                .sense(Sense::click()),
            );
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.on_hover_text("See it in Corners").clicked() {
                input.to_corner = Some((i, t, k));
            }
            ui.label(RichText::new(format!("+{loss:.2}")).size(13.0).color(DIM));
        });
    }
    row.col(|_| {});
}

impl App {
    /// The session in a few numbers across the top of Laps: the best lap, the
    /// best of every corner, consistency and pace, and an insight to look into.
    fn laps_figures(
        &mut self,
        ui: &mut Ui,
        session: &Session,
        model: Option<&crate::corner_model::CornerModel>,
    ) {
        use aim_xrk::notes::Note;
        self.open_session_notes(session);
        let notes = self
            .sensors
            .notes
            .as_ref()
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        let best = session.best_lap(MIN_LAP_MS).copied();
        let mut go = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 40.0;
            let figure = |ui: &mut Ui, label: &str, value: String, color: Color32, note: String| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(label).size(12.5).color(DIM));
                    ui.label(RichText::new(value).font(semibold(24.0)).color(color));
                    ui.label(RichText::new(note).size(12.5).color(SECONDARY));
                })
                .response
            };
            if let Some(b) = best {
                figure(ui, "Best lap", format_lap_time(b.duration_ms()), TEXT, format!("lap {}", b.number));
            }
            if let (Some(m), Some(b)) = (model, best) {
                let ideal = m.ideal_s();
                figure(ui, "Best of every corner", format_lap_time((ideal * 1000.0).round() as i64), TEXT, format!("{:.2} s on the table", (b.duration_ms() as f64 / 1000.0 - ideal).max(0.0)))
                    .on_hover_text("Your best time through every corner this session, added together");
            }
            for n in &notes {
                match n {
                    Note::Consistency { sd_s, laps } => {
                        figure(ui, "Consistency", format!("±{sd_s:.2} s"), TEXT, format!("{laps} racing laps"))
                            .on_hover_text("How much the racing laps varied (standard deviation; out/in laps and laps over 115% of the median left out). Smaller is more consistent.");
                    }
                    Note::LapTrend { per_lap_s, .. } => {
                        let slowed = *per_lap_s > 0.0;
                        figure(ui, "Pace", format!("{per_lap_s:+.2} s/lap"), if slowed { theme::FAVORITE } else { TEXT }, if slowed { "laps slowed".into() } else { "laps got faster".into() })
                            .on_hover_text("How lap times moved over the racing laps, on average per lap");
                    }
                    _ => {}
                }
            }
            // The corner that cost the most over the session, behind a lightbulb.
            if let Some(m) = model {
                let counting: Vec<usize> = (0..m.traces.len()).filter(|&t| m.counts[t]).collect();
                let avg = |k: usize| counting.iter().map(|&t| m.loss(t, k)).sum::<f64>() / counting.len().max(1) as f64;
                if let Some(k) = (0..m.segments.len()).max_by(|&a, &b| avg(a).total_cmp(&avg(b))).filter(|&k| avg(k) >= 0.02) {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    if resp.hovered() {
                        ui.painter().circle_filled(rect.center(), 18.0, SURFACE_HOVER);
                    }
                    Icon::Lightbulb.image(theme::FAVORITE, 20.0).paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(20.0)));
                    resp.on_hover_ui(|ui| {
                        ui.set_max_width(320.0);
                        ui.label(RichText::new("INSIGHT").font(semibold(11.0)).color(DIM).extra_letter_spacing(0.8));
                        ui.add_space(2.0);
                        ui.label(RichText::new(format!("Corner {} cost the most all session: {:.2} s a lap on average against the best you drove it.", k + 1, avg(k))).size(13.5).color(TEXT));
                        ui.add_space(4.0);
                        if ui.add(egui::Label::new(RichText::new("See it in Corners").size(13.5).color(theme::ACCENT)).sense(Sense::click())).clicked() {
                            go = Some(k);
                        }
                    });
                }
            }
        });
        if let (Some(k), Some(m)) = (go, model) {
            let t = self
                .analysis
                .lap
                .and_then(|i| m.trace_of(i))
                .unwrap_or(m.best);
            self.corners.selected = Some((t, k));
            self.analysis.analysis_view = AnalysisView::Corners;
        }
    }
}
