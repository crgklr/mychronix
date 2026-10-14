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

impl App {
    pub(super) fn report_view(&mut self, ui: &mut Ui, session: &Session) {
        let st = &mut self.report;
        // New session: out/in and slow laps start excluded, as RaceStudio leaves them out.
        if st.for_path != self.path {
            st.for_path = self.path.clone();
            let slow = slow_laps(session);
            st.excluded = session.laps.iter().zip(&slow).filter(|(_, s)| **s).map(|(l, _)| l.number).collect();
            st.plotted = 0;
        }
        let config = st.config.get_or_insert_with(ReportConfig::load);
        if config.columns.is_empty() {
            config.columns = Preset::Default.columns(session);
        }
        let columns: Vec<Column> = config.columns.iter().filter(|c| session.channels.iter().any(|ch| ch.name == c.channel)).cloned().collect();
        let mut changed = false;

        ui.label(
            RichText::new("Each lap boiled down to a few numbers, so you can see what the fast laps did differently: corner speed (speed min), top of the straight (RPM max), engine temperature, consistency.")
                .size(13.0)
                .color(SECONDARY),
        );
        ui.add_space(10.0);
        // Toolbar: presets, add a column.
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
            if chip(ui, "+ Column", st.adding.is_some()).clicked() {
                st.adding = if st.adding.is_some() { None } else { Some((String::new(), Stat::Max)) };
            }
        });
        if let Some((channel, stat)) = &mut st.adding {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                egui::ComboBox::from_id_salt("report_channel")
                    .selected_text(if channel.is_empty() { "Channel…".to_string() } else { report::short_name(channel).to_string() })
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for c in session.channels.iter().filter(|c| !is_dead(c)) {
                            let label = if report::short_name(&c.name) != c.name { format!("{} ({})", report::short_name(&c.name), c.name) } else { c.name.clone() };
                            ui.selectable_value(channel, c.name.clone(), label);
                        }
                    });
                for s in Stat::ALL {
                    if chip(ui, s.label(), *stat == s).clicked() {
                        *stat = s;
                    }
                }
                if !channel.is_empty() && button(ui, Some(Icon::Check), "Add", true).clicked() {
                    let col = Column { channel: channel.clone(), stat: *stat };
                    if !config.columns.contains(&col) {
                        config.columns.push(col);
                        changed = true;
                    }
                }
            });
        }
        ui.add_space(12.0);

        // Values: [lap][column], lap time first.
        let laps = &session.laps;
        let best = session.best_lap(MIN_LAP_MS).copied();
        let mut cells: Vec<Vec<Option<f64>>> = Vec::with_capacity(laps.len());
        for lap in laps {
            let mut row = vec![Some(lap.duration_ms() as f64 / 1000.0)];
            for c in &columns {
                row.push(report::cell(session, c, lap.start_ms, lap.end_ms));
            }
            cells.push(row);
        }
        let n_cols = columns.len() + 1;
        let included: Vec<usize> = (0..laps.len()).filter(|&i| !st.excluded.contains(&laps[i].number)).collect();
        let summaries: Vec<Option<[f64; 4]>> =
            (0..n_cols).map(|k| report::summary(&included.iter().filter_map(|&i| cells[i][k]).collect::<Vec<_>>())).collect();
        let decimals: Vec<usize> = std::iter::once(3)
            .chain(columns.iter().map(|c| if c.stat.is_share() { 0 } else { session.channels.iter().find(|ch| ch.name == c.channel).map(|ch| ch.decimals.min(2) as usize).unwrap_or(1) }))
            .collect();
        let units: Vec<String> = std::iter::once("s".to_string())
            .chain(columns.iter().map(|c| if c.stat.is_share() { "% of lap".to_string() } else { session.channels.iter().find(|ch| ch.name == c.channel).map(|ch| pretty_units(&ch.units).to_string()).unwrap_or_default() }))
            .collect();
        let fmt = |k: usize, v: f64| match k {
            0 => format_lap_time((v * 1000.0).round() as i64),
            _ if columns[k - 1].stat.is_share() => format!("{v:.0}%"),
            _ => format!("{v:.*}", decimals[k]),
        };

        let mut remove = None;
        let mut toggle = None;
        let mut hovered = None;
        let height = ui.available_height();
        // The graph keeps a readable height; the table gets the rest.
        let graph_h = 230.0;
        let table_h = (height - graph_h - 70.0).max(220.0);
        card(ui, |ui| {
            // Headers stay put while the laps scroll; wide tables scroll sideways as a whole.
            egui::ScrollArea::horizontal().id_salt("report_table_x").auto_shrink([false, true]).show(ui, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .id_salt("report_table")
                    .striped(false)
                    .resizable(false)
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(egui_extras::Column::auto().at_least(56.0))
                    .columns(egui_extras::Column::auto().at_least(64.0), n_cols)
                    // Fills the rest of the card, so the scrollbar sits at its edge.
                    .column(egui_extras::Column::remainder())
                    .min_scrolled_height(0.0)
                    .max_scroll_height(table_h)
                    .header(26.0, |mut header| {
                        // Click a title to plot it; × to remove.
                        header.col(|ui| {
                            ui.label(RichText::new("Lap").font(medium(13.0)).color(DIM));
                        });
                        for k in 0..n_cols {
                            header.col(|ui| {
                                let title = if k == 0 { "Lap time".to_string() } else { columns[k - 1].title() };
                                let color = if st.plotted == k { TEXT } else { SECONDARY };
                                let r = ui.add(egui::Label::new(RichText::new(&title).font(medium(13.0)).color(color)).sense(Sense::click()));
                                if r.on_hover_text(format!("Plot {title} by lap ({})", units[k])).clicked() {
                                    st.plotted = k;
                                }
                                if k > 0 && ui.add(Icon::X.image(DIM, 10.0).sense(Sense::click())).on_hover_text("Remove column").clicked() {
                                    remove = Some(k - 1);
                                }
                                ui.add_space(14.0);
                            });
                        }
                        header.col(|_| {});
                    })
                    .body(|mut body| {
                        // Statistics over the included laps.
                        for (j, name) in ["Max", "Min", "Average", "Std dev"].iter().enumerate() {
                            body.row(24.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(RichText::new(*name).font(medium(12.5)).color(DIM));
                                });
                                for k in 0..n_cols {
                                    row.col(|ui| {
                                        let text = summaries[k]
                                            .map(|s| if j == 3 && k == 0 { format!("{:.3}", s[3]) } else if j == 3 { format!("{:.*}", decimals[k] + 1, s[3]) } else { fmt(k, s[j]) })
                                            .unwrap_or_else(|| "—".into());
                                        ui.label(RichText::new(text).size(12.5).color(SECONDARY));
                                    });
                                }
                                row.col(|_| {});
                            });
                        }
                        for (i, lap) in laps.iter().enumerate() {
                            let on = !st.excluded.contains(&lap.number);
                            let is_best = Some(*lap) == best;
                            body.row(26.0, |mut row| {
                                row.col(|ui| {
                                    let (cb, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                                    ui.painter().rect_stroke(cb, CornerRadius::same(4), Stroke::new(1.3, if on { TEXT } else { DIM }), egui::StrokeKind::Inside);
                                    if on {
                                        Icon::Check.image(TEXT, 12.0).paint_at(ui, cb.shrink(2.0));
                                    }
                                    if resp.on_hover_text(if on { "Leave out of the statistics" } else { "Include in the statistics" }).clicked() {
                                        toggle = Some(lap.number);
                                    }
                                    if ui.label(RichText::new(lap.number.to_string()).font(medium(13.5)).color(if on { TEXT } else { DIM })).hovered() {
                                        hovered = Some(lap.number);
                                    }
                                });
                                for k in 0..n_cols {
                                    row.col(|ui| {
                                        let text = cells[i][k].map(|v| fmt(k, v)).unwrap_or_else(|| "—".into());
                                        let color = if !on {
                                            DIM
                                        } else if k == 0 && is_best {
                                            theme::GOOD
                                        } else {
                                            TEXT
                                        };
                                        let rt = RichText::new(text).font(if st.hovered_lap == Some(lap.number) { semibold(13.5) } else { regular(13.5) }).color(color);
                                        if ui.label(rt).hovered() {
                                            hovered = Some(lap.number);
                                        }
                                    });
                                }
                                row.col(|_| {});
                            });
                        }
                    });
            });
            ui.label(
                RichText::new(format!(
                    "Max, min, average and std dev are over the {} ticked laps of {}. Out, in and slow laps start unticked.",
                    included.len(),
                    laps.len()
                ))
                .size(12.0)
                .color(DIM),
            );
        });
        st.hovered_lap = hovered;
        if let Some(n) = toggle {
            if !st.excluded.remove(&n) {
                st.excluded.insert(n);
            }
        }

        // The plotted column against lap number.
        let k = st.plotted.min(n_cols - 1);
        let title = if k == 0 { "Lap time".to_string() } else { columns[k - 1].title() };
        ui.add_space(12.0);
        let pts: Vec<[f64; 2]> = included.iter().filter_map(|&i| cells[i][k].map(|v| [laps[i].number as f64, v])).collect();
        let hot: Vec<[f64; 2]> = st.hovered_lap.and_then(|n| laps.iter().position(|l| l.number == n)).and_then(|i| cells[i][k].map(|v| vec![[laps[i].number as f64, v]])).unwrap_or_default();
        graph_card(ui, graph_h, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{title} by lap")).font(medium(14.5)).color(TEXT));
                ui.label(RichText::new(&units[k]).size(13.0).color(DIM));
            });
            Plot::new(("report_plot", k))
                .height(ui.available_height())
                .show_background(false)
                .grid_color(GRID)
                .y_axis_min_width(46.0)
                .allow_zoom(false)
                .allow_drag(false)
                .allow_scroll(false)
                .show_x(false)
                .show_y(false)
                .x_axis_formatter(|m, _| if (m.value - m.value.round()).abs() < 1e-6 { format!("Lap {}", m.value.round()) } else { String::new() })
                .show(ui, |plot| {
                    plot.line(Line::new(title.clone(), PlotPoints::from(pts.clone())).color(SERIES[0]).width(2.0));
                    plot.points(egui_plot::Points::new("laps", PlotPoints::from(pts)).color(SERIES[0]).radius(4.0));
                    if !hot.is_empty() {
                        plot.points(egui_plot::Points::new("hovered", PlotPoints::from(hot)).color(TEXT).radius(7.0));
                    }
                });
        });

        if let Some(i) = remove {
            let col = columns[i].clone();
            config.columns.retain(|c| *c != col);
            st.plotted = 0;
            changed = true;
        }
        if changed {
            if let Err(e) = config.save() {
                self.error = Some(format!("Couldn't save the report columns: {e}"));
            }
        }
    }
}
