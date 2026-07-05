use eframe::egui;
use reqwest::Client;
use std::sync::mpsc::{channel, Receiver, Sender};

use crate::models::{AppConfig, CurseForgeConfig, DiscrepancyKind, DiscrepancyRecord, LocalConfig, ModInfo, ServerConfig};
use crate::progress::Progress;
use crate::state::AppState;
use crate::updater::IdentifyResult;
use crate::{config, local_mods, sftp, sync, updater};

pub enum Msg {
    Client(Result<IdentifyResult, String>),
    Server(usize, Result<(IdentifyResult, Vec<DiscrepancyRecord>), String>),
    Done(Result<String, String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Client,
    Server(usize),
    Settings,
}

fn default_config() -> AppConfig {
    AppConfig {
        local: LocalConfig {
            mods_folder: String::new(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
        },
        curseforge: None,
        servers: Vec::new(),
    }
}

#[derive(Default)]
struct ServerPanel {
    result: Option<IdentifyResult>,
    discrepancies: Vec<DiscrepancyRecord>,
    disc_sel: Vec<bool>,
    upd_sel: Vec<bool>,
}

pub struct App {
    cfg: Result<AppConfig, String>,
    state: AppState,
    rt: tokio::runtime::Runtime,
    http: Client,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    progress: Progress,
    busy: bool,
    log: Vec<String>,
    tab: Tab,
    client_result: Option<IdentifyResult>,
    client_sel: Vec<bool>,
    servers: Vec<ServerPanel>,
    draft: AppConfig,
    cf_key_draft: String,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = config::load_config("config.toml").map_err(|e| format!("{e:#}"));
        let n_servers = cfg.as_ref().map(|c| c.servers.len()).unwrap_or(0);
        let (tx, rx) = channel();
        let draft = cfg.as_ref().cloned().unwrap_or_else(|_| default_config());
        let cf_key_draft = draft.curseforge.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
        let tab = if cfg.is_ok() { Tab::Client } else { Tab::Settings };
        Self {
            cfg,
            state: AppState::load(),
            rt: tokio::runtime::Runtime::new().expect("tokio runtime"),
            http: Client::builder()
                .user_agent("mc-mod-updater/0.2")
                .build()
                .expect("http client"),
            tx,
            rx,
            progress: Progress::default(),
            busy: false,
            log: Vec::new(),
            tab,
            client_result: None,
            client_sel: Vec::new(),
            servers: (0..n_servers).map(|_| ServerPanel::default()).collect(),
            draft,
            cf_key_draft,
        }
    }

    fn cfg(&self) -> &AppConfig {
        self.cfg.as_ref().unwrap()
    }

    fn push_log(&mut self, s: impl Into<String>) {
        self.log.push(s.into());
        if self.log.len() > 200 {
            self.log.remove(0);
        }
    }

    // ── Background actions ────────────────────────────────────────────────

    fn scan_client(&mut self, ctx: &egui::Context) {
        let cfg = self.cfg().clone();
        let http = self.http.clone();
        let tx = self.tx.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        self.busy = true;
        self.push_log("Scanning client mods…");

        self.rt.spawn(async move {
            let res = async {
                progress.set("Hashing local mods…");
                let mods = local_mods::scan_local_folder(&cfg.local.mods_folder).await?;
                updater::identify_and_check_updates(
                    &http,
                    mods,
                    &cfg.local.minecraft_version,
                    &cfg.local.loader,
                    cfg.curseforge.as_ref().map(|c| c.api_key.as_str()),
                    &progress,
                )
                .await
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Client(res));
            ctx.request_repaint();
        });
    }

    fn apply_client_updates(&mut self, ctx: &egui::Context, selected: Vec<ModInfo>) {
        let cfg = self.cfg().clone();
        let http = self.http.clone();
        let tx = self.tx.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        self.busy = true;
        self.push_log(format!("Applying {} client update(s)…", selected.len()));

        self.rt.spawn(async move {
            let res = updater::apply_updates(&http, &selected, &cfg.local.mods_folder, &progress)
                .await
                .map(|_| "Client updates applied. Rescan to verify.".to_string())
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Done(res));
            ctx.request_repaint();
        });
    }

    fn scan_server(&mut self, ctx: &egui::Context, idx: usize) {
        let cfg = self.cfg().clone();
        let server = cfg.servers[idx].clone();
        let http = self.http.clone();
        let tx = self.tx.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        let client_known: Option<Vec<ModInfo>> =
            self.client_result.as_ref().map(|r| r.known.clone());
        self.busy = true;
        self.push_log(format!("Scanning server '{}'…", server_label(&server.name, idx)));

        self.rt.spawn(async move {
            let res = async {
                progress.set(format!("Connecting to {}…", server.host));
                let sftp = sftp::SftpClient::connect(&server).await?;
                let remote = sync::scan_remote_mods(&sftp, &server.remote_mods_folder, &progress).await?;
                let result = updater::identify_and_check_updates(
                    &http,
                    remote,
                    &cfg.local.minecraft_version,
                    &cfg.local.loader,
                    cfg.curseforge.as_ref().map(|c| c.api_key.as_str()),
                    &progress,
                )
                .await?;
                let discrepancies = match &client_known {
                    Some(known) => sync::compare_mod_sets(known, &result.known),
                    None => Vec::new(),
                };
                anyhow::Ok((result, discrepancies))
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Server(idx, res));
            ctx.request_repaint();
        });
    }

    fn apply_server_updates(&mut self, ctx: &egui::Context, idx: usize, selected: Vec<ModInfo>) {
        let server = self.cfg().servers[idx].clone();
        let http = self.http.clone();
        let tx = self.tx.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        self.busy = true;
        self.push_log(format!("Updating {} mod(s) on server…", selected.len()));

        self.rt.spawn(async move {
            let res = async {
                let sftp = sftp::SftpClient::connect(&server).await?;
                sync::update_server_mods(&http, &sftp, &selected, &server.remote_mods_folder, &progress).await
            }
            .await
            .map(|_| "Server mods updated. Rescan server to verify.".to_string())
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Done(res));
            ctx.request_repaint();
        });
    }

    fn apply_discrepancies(&mut self, ctx: &egui::Context, idx: usize, selected: Vec<DiscrepancyRecord>) {
        let server = self.cfg().servers[idx].clone();
        let tx = self.tx.clone();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        self.busy = true;
        self.push_log(format!("Syncing {} item(s) to server…", selected.len()));

        self.rt.spawn(async move {
            let res = async {
                let sftp = sftp::SftpClient::connect(&server).await?;
                sync::resolve_discrepancies(&sftp, &selected, &server.remote_mods_folder, &progress).await
            }
            .await
            .map(|_| "Sync complete. Rescan server to verify.".to_string())
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Done(res));
            ctx.request_repaint();
        });
    }

    // ── Message pump ──────────────────────────────────────────────────────

    fn pump(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = false;
            self.progress.set("");
            match msg {
                Msg::Client(Ok(res)) => {
                    self.client_sel = res.known.iter().map(|m| m.has_update()).collect();
                    self.push_log(format!(
                        "Client scan done: {} identified, {} unknown, {} update(s).",
                        res.known.len(),
                        res.unknown.len(),
                        res.known.iter().filter(|m| m.has_update()).count()
                    ));
                    self.client_result = Some(res);
                }
                Msg::Client(Err(e)) => self.push_log(format!("ERROR: {e}")),
                Msg::Server(idx, Ok((res, disc))) => {
                    let p = &mut self.servers[idx];
                    p.upd_sel = res.known.iter().map(|m| m.has_update()).collect();
                    p.disc_sel = vec![true; disc.len()];
                    self.push_log(format!(
                        "Server scan done: {} identified, {} update(s), {} discrepanc(ies).",
                        res.known.len(),
                        res.known.iter().filter(|m| m.has_update()).count(),
                        disc.len()
                    ));
                    let p = &mut self.servers[idx];
                    p.result = Some(res);
                    p.discrepancies = disc;
                }
                Msg::Server(_, Err(e)) => self.push_log(format!("ERROR: {e}")),
                Msg::Done(Ok(s)) => self.push_log(s),
                Msg::Done(Err(e)) => self.push_log(format!("ERROR: {e}")),
            }
        }
    }

    // ── UI ────────────────────────────────────────────────────────────────

    fn client_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("🔍 Scan client mods")).clicked() {
                self.scan_client(ctx);
            }
        });
        ui.separator();

        let Some(result) = self.client_result.clone() else {
            ui.label("No scan yet.");
            return;
        };

        let mut toggle_bl: Option<(String, String)> = None;
        let updatable: Vec<usize> = (0..result.known.len())
            .filter(|&i| result.known[i].has_update())
            .collect();

        ui.heading(format!("Updates available: {}", updatable.len()));
        egui::ScrollArea::vertical().id_salt("client_upd").max_height(300.0).show(ui, |ui| {
            egui::Grid::new("client_grid").striped(true).min_col_width(60.0).show(ui, |ui| {
                ui.strong(""); ui.strong("Mod"); ui.strong("Source"); ui.strong("Side");
                ui.strong("Current"); ui.strong("Latest"); ui.strong("Blacklist");
                ui.end_row();
                for &i in &updatable {
                    let m = &result.known[i];
                    let bl = self.state.is_blacklisted(&m.project_id);
                    if bl {
                        self.client_sel[i] = false;
                    }
                    ui.add_enabled(!bl, egui::Checkbox::without_text(&mut self.client_sel[i]));
                    ui.label(&m.project_name);
                    ui.label(m.source.label());
                    ui.label(m.side.label());
                    ui.label(&m.current_version.version_number);
                    ui.label(m.latest_version.as_ref().map(|v| v.version_number.as_str()).unwrap_or("-"));
                    if ui.button(if bl { "✔ blacklisted" } else { "🚫" }).clicked() {
                        toggle_bl = Some((m.project_id.clone(), m.project_name.clone()));
                    }
                    ui.end_row();
                }
            });
        });

        if let Some((pid, name)) = toggle_bl {
            self.state.toggle_blacklist(&pid, &name);
        }

        let selected: Vec<ModInfo> = updatable
            .iter()
            .filter(|&&i| self.client_sel[i] && !self.state.is_blacklisted(&result.known[i].project_id))
            .map(|&i| result.known[i].clone())
            .collect();

        if !updatable.is_empty()
            && ui
                .add_enabled(!self.busy && !selected.is_empty(),
                    egui::Button::new(format!("⬇ Apply {} update(s)", selected.len())))
                .clicked()
        {
            self.apply_client_updates(ctx, selected);
        }

        ui.separator();
        ui.collapsing(format!("Up to date ({})", result.known.len() - updatable.len()), |ui| {
            for m in result.known.iter().filter(|m| !m.has_update()) {
                ui.label(format!("{} — {} [{}]", m.project_name, m.current_version.version_number, m.source.label()));
            }
        });
        ui.collapsing(format!("Unknown mods ({})", result.unknown.len()), |ui| {
            for u in &result.unknown {
                ui.label(&u.local_mod.filename);
            }
        });
    }

    fn server_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, idx: usize) {
        let server_name = server_label(&self.cfg().servers[idx].name, idx).to_string();
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.busy, egui::Button::new(format!("🔍 Scan server '{server_name}'"))).clicked() {
                self.scan_server(ctx, idx);
            }
            if self.client_result.is_none() {
                ui.label("(no client scan → only server-side update check, no client comparison)");
            }
        });
        ui.separator();

        let Some(result) = self.servers[idx].result.clone() else {
            ui.label("No scan yet. Works standalone — client scan not required.");
            return;
        };

        let mut toggle_bl: Option<(String, String)> = None;

        // ── Server-side updates (includes server-only mods) ──
        let updatable: Vec<usize> = (0..result.known.len())
            .filter(|&i| result.known[i].has_update())
            .collect();
        ui.heading(format!("Server mod updates: {}", updatable.len()));
        egui::ScrollArea::vertical().id_salt("srv_upd").max_height(250.0).show(ui, |ui| {
            egui::Grid::new(("srv_grid", idx)).striped(true).min_col_width(60.0).show(ui, |ui| {
                ui.strong(""); ui.strong("Mod"); ui.strong("Source"); ui.strong("Side");
                ui.strong("Current"); ui.strong("Latest"); ui.strong("Blacklist");
                ui.end_row();
                for &i in &updatable {
                    let m = &result.known[i];
                    let bl = self.state.is_blacklisted(&m.project_id);
                    if bl {
                        self.servers[idx].upd_sel[i] = false;
                    }
                    ui.add_enabled(!bl, egui::Checkbox::without_text(&mut self.servers[idx].upd_sel[i]));
                    ui.label(&m.project_name);
                    ui.label(m.source.label());
                    ui.label(m.side.label());
                    ui.label(&m.current_version.version_number);
                    ui.label(m.latest_version.as_ref().map(|v| v.version_number.as_str()).unwrap_or("-"));
                    if ui.button(if bl { "✔ blacklisted" } else { "🚫" }).clicked() {
                        toggle_bl = Some((m.project_id.clone(), m.project_name.clone()));
                    }
                    ui.end_row();
                }
            });
        });

        if let Some((pid, name)) = toggle_bl {
            self.state.toggle_blacklist(&pid, &name);
        }

        let sel_updates: Vec<ModInfo> = updatable
            .iter()
            .filter(|&&i| self.servers[idx].upd_sel[i] && !self.state.is_blacklisted(&result.known[i].project_id))
            .map(|&i| result.known[i].clone())
            .collect();
        if !updatable.is_empty()
            && ui
                .add_enabled(!self.busy && !sel_updates.is_empty(),
                    egui::Button::new(format!("⬆ Update {} mod(s) on server", sel_updates.len())))
                .clicked()
        {
            self.apply_server_updates(ctx, idx, sel_updates);
        }

        ui.separator();

        // ── Discrepancies vs client ──
        if self.client_result.is_none() {
            ui.label("Scan client first to compare client ↔ server.");
        } else {
            let disc = self.servers[idx].discrepancies.clone();
            ui.heading(format!("Discrepancies vs client: {}", disc.len()));
            egui::ScrollArea::vertical().id_salt("srv_disc").max_height(250.0).show(ui, |ui| {
                egui::Grid::new(("disc_grid", idx)).striped(true).min_col_width(60.0).show(ui, |ui| {
                    ui.strong(""); ui.strong("Mod"); ui.strong("Kind"); ui.strong("Action");
                    ui.end_row();
                    for (i, d) in disc.iter().enumerate() {
                        let bl = self.state.is_blacklisted(&d.project_id);
                        if bl {
                            self.servers[idx].disc_sel[i] = false;
                        }
                        ui.add_enabled(!bl, egui::Checkbox::without_text(&mut self.servers[idx].disc_sel[i]));
                        ui.label(&d.project_name);
                        let (kind, action) = match d.kind {
                            DiscrepancyKind::Mismatch => ("version mismatch", "upload client version"),
                            DiscrepancyKind::ClientOnly => ("missing on server", "upload to server"),
                            DiscrepancyKind::ServerOnly => ("extra on server", "delete from server"),
                        };
                        ui.label(kind);
                        ui.label(if bl { "blacklisted — skipped" } else { action });
                        ui.end_row();
                    }
                });
            });

            let sel_disc: Vec<DiscrepancyRecord> = disc
                .iter()
                .enumerate()
                .filter(|(i, d)| self.servers[idx].disc_sel[*i] && !self.state.is_blacklisted(&d.project_id))
                .map(|(_, d)| d.clone())
                .collect();
            if !disc.is_empty()
                && ui
                    .add_enabled(!self.busy && !sel_disc.is_empty(),
                        egui::Button::new(format!("🔄 Sync {} item(s)", sel_disc.len())))
                    .clicked()
            {
                self.apply_discrepancies(ctx, idx, sel_disc);
            }
        }
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.add_space(6.0);

        egui::Grid::new("set_local").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Mods folder");
            ui.add(egui::TextEdit::singleline(&mut self.draft.local.mods_folder).desired_width(500.0));
            ui.end_row();
            ui.label("Minecraft version");
            ui.text_edit_singleline(&mut self.draft.local.minecraft_version);
            ui.end_row();
            ui.label("Loader");
            egui::ComboBox::from_id_salt("loader")
                .selected_text(&self.draft.local.loader)
                .show_ui(ui, |ui| {
                    for l in ["neoforge", "forge", "fabric", "quilt"] {
                        ui.selectable_value(&mut self.draft.local.loader, l.to_string(), l);
                    }
                });
            ui.end_row();
            ui.label("CurseForge API key");
            ui.add(egui::TextEdit::singleline(&mut self.cf_key_draft).desired_width(500.0)
                .hint_text("empty = Modrinth only"));
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.heading("Servers");
            if ui.button("➕ Add server").clicked() {
                self.draft.servers.push(ServerConfig {
                    name: format!("Server {}", self.draft.servers.len() + 1),
                    host: String::new(),
                    port: 22,
                    username: String::new(),
                    password: String::new(),
                    remote_mods_folder: "/mods".into(),
                });
            }
        });

        let mut remove: Option<usize> = None;
        for (i, s) in self.draft.servers.iter_mut().enumerate() {
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::Grid::new(("set_srv", i)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut s.name);
                    ui.end_row();
                    ui.label("Host");
                    ui.text_edit_singleline(&mut s.host);
                    ui.end_row();
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut s.port).range(1..=65535));
                    ui.end_row();
                    ui.label("Username");
                    ui.text_edit_singleline(&mut s.username);
                    ui.end_row();
                    ui.label("Password");
                    ui.add(egui::TextEdit::singleline(&mut s.password).password(true));
                    ui.end_row();
                    ui.label("Remote mods folder");
                    ui.text_edit_singleline(&mut s.remote_mods_folder);
                    ui.end_row();
                });
                if ui.button("🗑 Remove server").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.draft.servers.remove(i);
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("💾 Save").clicked() {
                self.draft.curseforge = if self.cf_key_draft.trim().is_empty() {
                    None
                } else {
                    Some(CurseForgeConfig { api_key: self.cf_key_draft.trim().to_string() })
                };
                match config::save_config("config.toml", &self.draft) {
                    Ok(()) => {
                        self.cfg = Ok(self.draft.clone());
                        // Server list may have changed: reset panels, keep client scan.
                        self.servers = (0..self.draft.servers.len())
                            .map(|_| ServerPanel::default())
                            .collect();
                        self.push_log("Settings saved to config.toml.");
                    }
                    Err(e) => self.push_log(format!("ERROR saving config: {e:#}")),
                }
            }
            if ui.button("↩ Revert").clicked() {
                if let Ok(c) = &self.cfg {
                    self.draft = c.clone();
                    self.cf_key_draft = c.curseforge.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
                }
            }
        });
    }

    fn blacklist_panel(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(format!("Blacklist ({})", self.state.blacklist.len()), |ui| {
            let entries: Vec<(String, String)> = self
                .state
                .blacklist
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (pid, name) in entries {
                ui.horizontal(|ui| {
                    if ui.button("❌").clicked() {
                        self.state.toggle_blacklist(&pid, &name);
                    }
                    ui.label(name);
                });
            }
        });
    }
}

fn server_label(name: &str, idx: usize) -> String {
    if name.is_empty() {
        format!("Server {}", idx + 1)
    } else {
        name.to_string()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();

        let cfg_ok = self.cfg.is_ok();

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if cfg_ok {
                    ui.selectable_value(&mut self.tab, Tab::Client, "🖥 Client");
                    let names: Vec<String> = self
                        .cfg()
                        .servers
                        .iter()
                        .enumerate()
                        .map(|(i, s)| server_label(&s.name, i))
                        .collect();
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.tab, Tab::Server(i), format!("🌐 {n}"));
                    }
                }
                ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Settings");
            });
        });

        if !cfg_ok {
            self.tab = Tab::Settings;
        }

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                    ui.label(self.progress.get());
                    ctx.request_repaint_after(std::time::Duration::from_millis(150));
                } else if let Some(last) = self.log.last() {
                    ui.label(last);
                }
            });
            ui.collapsing("Log", |ui| {
                egui::ScrollArea::vertical().max_height(120.0).stick_to_bottom(true).show(ui, |ui| {
                    for line in &self.log {
                        ui.label(line);
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.tab {
                    Tab::Client => self.client_tab(ui, ctx),
                    Tab::Server(idx) if idx < self.servers.len() => self.server_tab(ui, ctx, idx),
                    Tab::Server(_) => {}
                    Tab::Settings => {
                        if let Err(e) = self.cfg.clone() {
                            ui.colored_label(egui::Color32::LIGHT_RED, format!("Config error: {e}"));
                            ui.label("Fill in the settings below and save.");
                            ui.separator();
                        }
                        self.settings_tab(ui);
                    }
                }
                if self.tab != Tab::Settings {
                    ui.separator();
                    self.blacklist_panel(ui);
                }
            });
        });
    }
}
