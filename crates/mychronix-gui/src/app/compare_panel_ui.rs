//! Compare's side panel when there's no map (no GPS): each lap's time and
//! gap, where along the lap time was won and lost, and its peaks.

use super::*;

/// Stretches of equal distance the lap is split into.
const STRETCHES: usize = 8;

/// The gap (s) at `d` meters, from `time_gap`'s [meters, seconds] samples.
fn gap_at(gaps: &[[f64; 2]], d: f64) -> f64 {
    let i = gaps.partition_point(|p| p[0] < d).clamp(1, gaps.len().max(2) - 1);
    let (a, b) = (gaps[i - 1], gaps[i]);
    let f = if b[0] > a[0] { ((d - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0) } else { 0.0 };
    a[1] + (b[1] - a[1]) * f
}

/// "+0.12" / "−0.08" seconds.
fn signed(s: f64) -> String {
    if s >= 0.0 { format!("+{s:.2}") } else { format!("−{:.2}", -s) }
}

impl App {
    pub(super) fn compare_breakdown(&mut self, ui: &mut Ui, reference: &crate::compare::DistanceProfile) {
        let laps = self.compare.clone();
        let Some(ref_lap) = laps.first() else { return };
        let ref_ms = ref_lap.lap.duration_ms();
        let ref_len = reference.length();
        Frame::new().fill(SURFACE).corner_radius(CornerRadius::same(theme::RADIUS)).inner_margin(Margin::same(16)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;

            // The laps: time and gap to the reference.
            ui.label(RichText::new("LAPS").font(semibold(12.0)).color(DIM).extra_letter_spacing(0.8));
            for c in &laps {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(3.0, SERIES[c.slot]));
                    ui.label(RichText::new(format_lap_time(c.lap.duration_ms())).font(semibold(16.0)).color(TEXT));
                    let d = (c.lap.duration_ms() - ref_ms) as f64 / 1000.0;
                    let (text, color) = if c.slot == ref_lap.slot { ("reference".to_string(), DIM) } else { (signed(d), if d <= 0.0 { theme::GOOD } else { theme::DANGER }) };
                    ui.label(RichText::new(text).font(medium(14.0)).color(color));
                });
                ui.label(RichText::new(&c.label).font(regular(12.5)).color(DIM));
            }
            ui.add_space(10.0);

            // Where the others won and lost time, stretch by stretch.
            let others: Vec<(usize, Vec<[f64; 2]>)> =
                laps[1..].iter().filter_map(|c| Some((c.slot, crate::compare::time_gap(c.profile.as_ref()?, reference, 600)))).collect();
            if !others.is_empty() && ref_len > 0.0 {
                ui.label(RichText::new("WHERE TIME WAS WON AND LOST").font(semibold(12.0)).color(DIM).extra_letter_spacing(0.8));
                ui.label(RichText::new(format!("Each lap against the reference, in {STRETCHES} equal stretches of about {:.0} m.", ref_len / STRETCHES as f64)).font(regular(12.5)).color(SECONDARY));
                let deltas: Vec<Vec<f64>> = others
                    .iter()
                    .map(|(_, g)| (0..STRETCHES).map(|k| gap_at(g, ref_len * (k + 1) as f64 / STRETCHES as f64) - gap_at(g, ref_len * k as f64 / STRETCHES as f64)).collect())
                    .collect();
                let scale = deltas.iter().flatten().map(|d| d.abs()).fold(0.0_f64, f64::max).max(0.01);
                let row_h = 14.0 * others.len() as f32 + 10.0;
                for k in 0..STRETCHES {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                    let p = ui.painter();
                    p.text(pos2(rect.left(), rect.center().y), egui::Align2::LEFT_CENTER, format!("{}", k + 1), medium(13.0), SECONDARY);
                    // Bars grow right (lost time, red) or left (gained, green) from the middle.
                    let (left, right) = (rect.left() + 24.0, rect.right() - 56.0);
                    let mid = (left + right) / 2.0;
                    p.line_segment([pos2(mid, rect.top() + 2.0), pos2(mid, rect.bottom() - 2.0)], Stroke::new(1.0, theme::OUTLINE));
                    for (j, (slot, _)) in others.iter().enumerate() {
                        let d = deltas[j][k];
                        let w = (d.abs() / scale) as f32 * (right - mid);
                        let y = rect.top() + 5.0 + 14.0 * j as f32;
                        let bar = if d >= 0.0 { Rect::from_min_size(pos2(mid, y), vec2(w, 10.0)) } else { Rect::from_min_size(pos2(mid - w, y), vec2(w, 10.0)) };
                        p.rect_filled(bar, CornerRadius::same(3), if d >= 0.0 { theme::DANGER } else { theme::GOOD });
                        p.rect_filled(Rect::from_min_size(pos2(left - 10.0, y + 2.0), vec2(4.0, 6.0)), CornerRadius::same(1), SERIES[*slot]);
                        p.text(pos2(rect.right(), y + 5.0), egui::Align2::RIGHT_CENTER, signed(d), regular(12.0), SECONDARY);
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("■ gained").font(regular(12.0)).color(theme::GOOD));
                    ui.label(RichText::new("■ lost").font(regular(12.0)).color(theme::DANGER));
                });
                ui.add_space(10.0);
            }

            // Peaks in each lap.
            let peak = |c: &crate::compare::CompareLap, name: &str| -> Option<(f64, f64, String)> {
                let ch = same_channel(&c.session, name)?;
                let v: Vec<f64> = ch.times_ms.iter().zip(&ch.values).filter(|(t, _)| (c.lap.start_ms..=c.lap.end_ms).contains(t)).map(|(_, v)| *v).collect();
                let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |a, &x| (a.0.min(x), a.1.max(x)));
                (!v.is_empty()).then(|| (lo, hi, pretty_units(&ch.units).to_string()))
            };
            ui.label(RichText::new("PEAKS").font(semibold(12.0)).color(DIM).extra_letter_spacing(0.8));
            egui::Grid::new("compare_peaks").num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
                for h in ["", "Top RPM", "Low RPM", "Top speed"] {
                    ui.label(RichText::new(h).font(medium(12.5)).color(DIM));
                }
                ui.end_row();
                for c in &laps {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 16.0), Sense::hover());
                    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(3.0, SERIES[c.slot]));
                    let rpm = peak(c, "RPM");
                    ui.label(RichText::new(rpm.as_ref().map(|r| format!("{:.0}", r.1)).unwrap_or_else(|| "–".into())).font(regular(14.0)).color(TEXT));
                    ui.label(RichText::new(rpm.as_ref().map(|r| format!("{:.0}", r.0)).unwrap_or_else(|| "–".into())).font(regular(14.0)).color(TEXT));
                    let speed = peak(c, "GPS Speed").map(|s| format!("{:.0} {}", s.1, s.2)).unwrap_or_else(|| "–".into());
                    ui.label(RichText::new(speed).font(regular(14.0)).color(TEXT));
                    ui.end_row();
                }
            });
        });
    }
}
