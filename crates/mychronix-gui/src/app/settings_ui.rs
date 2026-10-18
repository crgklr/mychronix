//! The Settings page: units, clock, what Mychronix fetches from the internet,
//! and the Mychronix Performance Engineer (the MCP server an AI assistant on
//! this computer can use). Changes are saved to `~/Mychronix/settings.json`
//! and take effect at once.

use super::*;
use super::sessions_filter_ui::{section, segments_in, switch_row};

/// A change to make to the settings, once the page has drawn.
type Change = Option<Box<dyn FnOnce(&mut crate::settings::Settings)>>;

#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Auto,
    A,
    B,
}

impl App {
    pub(super) fn settings_page(&mut self, ui: &mut Ui) {
        let s = crate::settings::get();
        let mut change: Change = None;

        ui.label(RichText::new("Settings").font(semibold(28.0)).color(TEXT));
        ui.label(RichText::new("Saved on this computer, in your Mychronix folder. Changes take effect right away.").font(regular(14.5)).color(SECONDARY));
        ui.add_space(18.0);

        egui::ScrollArea::vertical().id_salt("settings_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let w = ui.available_width().min(760.0);
            ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing.y = 14.0;

                // Units
                let mut units = match s.units.as_deref() {
                    Some("imperial") => Choice::A,
                    Some("metric") => Choice::B,
                    _ => Choice::Auto,
                };
                let now = if crate::units::imperial() { "mph, °F and feet" } else { "km/h, °C and meters" };
                section(ui, Icon::Gauge, "Units", &format!("Showing {now}"), false, |ui| {
                    let before = units;
                    segments_in(ui, &mut units, &[(Choice::Auto, "From your region"), (Choice::A, "mph · °F · ft"), (Choice::B, "km/h · °C · m")], 3);
                    if units != before {
                        change = Some(Box::new(move |s| {
                            s.units = match units {
                                Choice::A => Some("imperial".into()),
                                Choice::B => Some("metric".into()),
                                Choice::Auto => None,
                            }
                        }));
                    }
                });

                // Clock
                let mut clock = match s.clock.as_deref() {
                    Some("12") => Choice::A,
                    Some("24") => Choice::B,
                    _ => Choice::Auto,
                };
                let example = crate::events::clock_time("14:19");
                section(ui, Icon::Clock, "Clock", &format!("Times read like {example}"), false, |ui| {
                    let before = clock;
                    segments_in(ui, &mut clock, &[(Choice::Auto, "From your region"), (Choice::A, "12-hour  ·  2:19 PM"), (Choice::B, "24-hour  ·  14:19")], 3);
                    if clock != before {
                        change = Some(Box::new(move |s| {
                            s.clock = match clock {
                                Choice::A => Some("12".into()),
                                Choice::B => Some("24".into()),
                                Choice::Auto => None,
                            }
                        }));
                    }
                });

                // Internet
                section(ui, Icon::CloudOff, "Internet", "Mychronix works offline; with a connection it can also fetch these", false, |ui| {
                    if switch_row(ui, s.weather, "Weather for each session", "From Open-Meteo: sends the track's location and the date, nothing else") {
                        let v = !s.weather;
                        change = Some(Box::new(move |s| s.weather = v));
                    }
                    if switch_row(ui, s.aim_tracks, "AiM's track database", "Every AiM track with its outline, from AiM's server; then only what changed") {
                        let v = !s.aim_tracks;
                        change = Some(Box::new(move |s| s.aim_tracks = v));
                    }
                    if switch_row(ui, s.update_check, "Check for a newer Mychronix", "Asks GitHub once a day; downloading is up to you") {
                        let v = !s.update_check;
                        change = Some(Box::new(move |s| s.update_check = v));
                    }
                });

                // Mychronix Performance Engineer
                self.mcp_section(ui, s.mcp, &mut change);
            });
        });

        if let Some(f) = change {
            if let Err(e) = crate::settings::change(f) {
                self.error = Some(format!("Couldn't save the settings: {e}"));
            }
            // Units and the clock show everywhere: draw everything again in them.
            self.library = None;
            ui.ctx().request_repaint();
        }
    }

    fn mcp_section(&mut self, ui: &mut Ui, on: bool, change: &mut Change) {
        let summary = if on { "On: your AI assistant can read your sessions" } else { "Off" };
        // (Not "active": the section's Clear link is for filters.)
        section(ui, Icon::Activity, "Mychronix Performance Engineer (MCP)", summary, false, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Let the AI assistant on your computer (Claude Desktop, Codex, or another app that supports MCP) read your Mychronix sessions and coach from them. Ask how a race day went, where the time is in each corner, or what changed since last week. Stop paying a subscription for a dedicated AI racing coach and use your own.",
                    )
                    .size(14.0)
                    .color(TEXT),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            if switch_row(ui, on, "Let my AI assistant read my sessions", "Read-only: it can't change your data or talk to a MyChron") {
                let v = !on;
                *change = Some(Box::new(move |s| s.mcp = v));
            }
            ui.add(
                egui::Label::new(
                    RichText::new("Mychronix answers only the assistant on this computer and sends nothing itself. What the assistant reads goes to the company that runs it (Anthropic for Claude, OpenAI for Codex, and so on), like anything else you share in that chat.")
                        .size(12.5)
                        .color(DIM),
                )
                .wrap(),
            );
            if !on {
                return;
            }
            ui.add_space(8.0);
            let command = mcp_command();
            ui.label(RichText::new("Connect it").font(medium(14.5)).color(TEXT));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if button(ui, Some(Icon::Plus), "Add to Claude Desktop", true).on_hover_text("Adds Mychronix to Claude Desktop's settings, keeping everything else in them").clicked() {
                    self.mcp_message = Some(match add_to_claude(&command) {
                        Ok(path) => (true, format!("Added to Claude Desktop ({}). Quit and reopen Claude Desktop, then look for Mychronix in its tools.", path.display())),
                        Err(e) => (false, e),
                    });
                }
                if button(ui, Some(Icon::Plus), "Add to Codex", true).on_hover_text("Adds Mychronix to Codex's settings (~/.codex/config.toml), keeping everything else in them").clicked() {
                    self.mcp_message = Some(match add_to_codex(&command) {
                        Ok(path) => (true, format!("Added to Codex ({}). Restart Codex, then look for Mychronix in its MCP servers.", path.display())),
                        Err(e) => (false, e),
                    });
                }
                if button(ui, None, "Copy the command", false).on_hover_text("For other assistant apps: add an MCP server that runs this").clicked() {
                    ui.ctx().copy_text(command.clone());
                    self.mcp_message = Some((true, "Copied. In your assistant app, add an MCP server (\"stdio\" or \"command\") that runs it.".into()));
                }
            });
            if let Some((ok, m)) = &self.mcp_message {
                ui.add(egui::Label::new(RichText::new(m).size(13.0).color(if *ok { theme::GOOD } else { theme::DANGER })).wrap());
            }
            ui.add(egui::Label::new(RichText::new(format!("Other apps: add an MCP server that runs  {command}")).size(12.5).color(DIM)).wrap());
        });
    }
}

/// How an assistant app starts the Mychronix Performance Engineer: this
/// program, with `--mcp`.
fn mcp_command() -> String {
    // An AppImage runs from a temporary mount that changes every launch: point at the AppImage itself.
    let exe = std::env::var("APPIMAGE")
        .ok()
        .filter(|p| !p.is_empty())
        .or_else(|| std::env::current_exe().ok().map(|p| p.display().to_string()))
        .unwrap_or_else(|| "mychronix-gui".into());
    format!("{exe} --mcp")
}

/// Codex's settings file: `$CODEX_HOME/config.toml`, else `~/.codex/config.toml`.
fn codex_config() -> Option<PathBuf> {
    let dir = std::env::var_os("CODEX_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".codex")))?;
    Some(dir.join("config.toml"))
}

/// Adds (or updates) `[mcp_servers.mychronix]` in Codex's settings, keeping
/// everything else as it was, with a backup of the file first.
fn add_to_codex(command: &str) -> Result<PathBuf, String> {
    let path = codex_config().ok_or("No home folder")?;
    if !path.parent().is_some_and(|d| d.exists()) {
        return Err("Codex doesn't seem to be installed here (its settings folder isn't there). Install it, open it once, then try again.".into());
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Couldn't read {}: {e}", path.display())),
    };
    if path.exists() {
        std::fs::copy(&path, path.with_extension("toml.before-mychronix")).map_err(|e| format!("Couldn't back up {}: {e}. Nothing was changed.", path.display()))?;
    }
    let (program, _) = command.rsplit_once(" --mcp").unwrap_or((command, ""));
    crate::storage::write_atomic(&path, with_codex_server(&text, program).as_bytes()).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    log::info!("added the Mychronix MCP server to {}", path.display());
    Ok(path)
}

/// `config` with `[mcp_servers.mychronix]` running `program --mcp`: the table
/// replaced if it's there, else added at the end. Nothing else changes.
fn with_codex_server(config: &str, program: &str) -> String {
    let quoted = format!("\"{}\"", program.replace('\\', "\\\\").replace('"', "\\\""));
    let table = format!("[mcp_servers.mychronix]\ncommand = {quoted}\nargs = [\"--mcp\"]\n");
    let mut out = String::new();
    let mut skipping = false;
    let mut replaced = false;
    for line in config.lines() {
        let header = line.trim_start().starts_with('[');
        if header && line.trim() == "[mcp_servers.mychronix]" {
            out.push_str(&table);
            skipping = true;
            replaced = true;
            continue;
        }
        if skipping && header {
            skipping = false;
            out.push('\n');
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&table);
    }
    out
}

/// Claude Desktop's settings file on this system.
fn claude_config() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(if cfg!(target_os = "macos") { home.join("Library/Application Support/Claude/claude_desktop_config.json") } else { home.join(".config/Claude/claude_desktop_config.json") })
}

/// Adds Mychronix to Claude Desktop's MCP servers, keeping everything else,
/// with a backup of the file as it was.
fn add_to_claude(command: &str) -> Result<PathBuf, String> {
    let path = claude_config().ok_or("No home folder")?;
    let mut config: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("Claude Desktop's settings ({}) couldn't be read: {e}. Nothing was changed.", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !path.parent().is_some_and(|d| d.exists()) {
                return Err("Claude Desktop doesn't seem to be installed here (its settings folder isn't there). Install it, open it once, then try again.".into());
            }
            serde_json::json!({})
        }
        Err(e) => return Err(format!("Couldn't read {}: {e}", path.display())),
    };
    if path.exists() {
        let backup = path.with_extension("json.before-mychronix");
        std::fs::copy(&path, &backup).map_err(|e| format!("Couldn't back up {}: {e}. Nothing was changed.", path.display()))?;
    }
    let (program, _) = command.rsplit_once(" --mcp").unwrap_or((command, ""));
    let servers = config.as_object_mut().ok_or("Claude Desktop's settings aren't what Mychronix expects. Nothing was changed.")?.entry("mcpServers").or_insert_with(|| serde_json::json!({}));
    let servers = servers.as_object_mut().ok_or("Claude Desktop's settings aren't what Mychronix expects. Nothing was changed.")?;
    servers.insert("mychronix".into(), serde_json::json!({ "command": program, "args": ["--mcp"] }));
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    crate::storage::write_atomic(&path, text.as_bytes()).map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    log::info!("added the Mychronix MCP server to {}", path.display());
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_settings_keep_everything_else() {
        let before = "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"npx\"\n";
        let after = with_codex_server(before, "/opt/Mychronix/mychronix-gui");
        assert!(after.starts_with(before));
        assert!(after.ends_with("[mcp_servers.mychronix]\ncommand = \"/opt/Mychronix/mychronix-gui\"\nargs = [\"--mcp\"]\n"));
        // Added again: replaced in place, not duplicated, and what follows survives.
        let again = with_codex_server("[mcp_servers.mychronix]\ncommand = \"/old\"\nargs = []\n\n[x]\ny = 1\n", "/new");
        assert_eq!(again.matches("[mcp_servers.mychronix]").count(), 1);
        assert!(again.contains("command = \"/new\"") && !again.contains("/old") && again.contains("[x]\ny = 1"));
    }
}
