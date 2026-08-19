#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use global_hotkey::{
    hotkey::{Code, HotKey},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
enum ClickButton {
    Left,
    Middle,
    Right,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
enum ClickMode {
    Hold,
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct AppConfig {
    cps: u32,
    button: ClickButton,
    mode: ClickMode,
    toggle_key: Code,
    kill_key: Code,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cps: 10,
            button: ClickButton::Left,
            mode: ClickMode::Toggle,
            toggle_key: Code::F6,
            kill_key: Code::F7,
        }
    }
}

struct AutoClickerApp {
    config: Arc<RwLock<AppConfig>>,
    is_clicking: Arc<AtomicBool>,
    hotkey_manager: GlobalHotKeyManager,
    toggle_hotkey: HotKey,
    kill_hotkey: HotKey,
    config_path: PathBuf,
}

impl AutoClickerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Set a darker theme by default
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let config_path = directories::ProjectDirs::from("com", "autoclickrs", "autoclickrs")
            .map(|proj| {
                let path = proj.config_dir().to_path_buf();
                let _ = std::fs::create_dir_all(&path);
                path.join("config.json")
            })
            .unwrap_or_else(|| PathBuf::from("config.json"));

        let config_data: AppConfig = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let hotkey_manager = GlobalHotKeyManager::new().unwrap();
        let toggle_hotkey = HotKey::new(None, config_data.toggle_key);
        let kill_hotkey = HotKey::new(None, config_data.kill_key);

        let _ = hotkey_manager.register(toggle_hotkey);
        let _ = hotkey_manager.register(kill_hotkey);

        let config = Arc::new(RwLock::new(config_data));
        let is_clicking = Arc::new(AtomicBool::new(false));
        
        let config_clone = config.clone();
        let is_clicking_clone = is_clicking.clone();

        // Spawn clicker thread
        thread::spawn(move || {
            loop {
                if is_clicking_clone.load(Ordering::Relaxed) {
                    let (down_flag, up_flag, cps) = {
                        let cfg = config_clone.read().unwrap();
                        let (d, u) = match cfg.button {
                            ClickButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                            ClickButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                            ClickButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                        };
                        (d, u, cfg.cps)
                    };

                    // IMPROVED CLICK REGISTRATION:
                    // We add a "hold" delay between down and up events.
                    // This ensures games poll the "down" state before the "up" occurs.
                    let total_period = Duration::from_secs_f64(1.0 / (cps as f64).max(0.1));
                    let hold_time = total_period.min(Duration::from_millis(15)) / 2;

                    send_mouse_input(down_flag);
                    thread::sleep(hold_time);
                    send_mouse_input(up_flag);
                    
                    // Wait the remaining time of the period
                    thread::sleep(total_period.saturating_sub(hold_time));
                } else {
                    thread::sleep(Duration::from_millis(10));
                }
            }
        });

        Self {
            config,
            is_clicking,
            hotkey_manager,
            toggle_hotkey,
            kill_hotkey,
            config_path,
        }
    }

    fn save_config(&self) {
        let cfg = self.config.read().unwrap();
        if let Ok(s) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(&self.config_path, s);
        }
    }

    fn update_hotkeys(&mut self) {
        let _ = self.hotkey_manager.unregister(self.toggle_hotkey);
        let _ = self.hotkey_manager.unregister(self.kill_hotkey);
        
        let cfg = self.config.read().unwrap();
        self.toggle_hotkey = HotKey::new(None, cfg.toggle_key);
        self.kill_hotkey = HotKey::new(None, cfg.kill_key);
        
        let _ = self.hotkey_manager.register(self.toggle_hotkey);
        let _ = self.hotkey_manager.register(self.kill_hotkey);
    }
}

fn send_mouse_input(flags: u32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

impl eframe::App for AutoClickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle Hotkeys
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == self.toggle_hotkey.id() {
                let mode = self.config.read().unwrap().mode;
                match mode {
                    ClickMode::Hold => {
                        self.is_clicking.store(event.state == HotKeyState::Pressed, Ordering::Relaxed);
                    }
                    ClickMode::Toggle => {
                        if event.state == HotKeyState::Pressed {
                            let current = self.is_clicking.load(Ordering::Relaxed);
                            self.is_clicking.store(!current, Ordering::Relaxed);
                        }
                    }
                }
            } else if event.id == self.kill_hotkey.id() {
                if event.state == HotKeyState::Pressed {
                    self.is_clicking.store(false, Ordering::Relaxed);
                }
            }
            ctx.request_repaint();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading("AutoClickrs");
                ui.add_space(12.0);
            });

            let mut changed = false;
            {
                let mut cfg = self.config.write().unwrap();

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Click Speed").strong());
                    ui.horizontal(|ui| {
                        let old_cps = cfg.cps;
                        ui.add(egui::Slider::new(&mut cfg.cps, 1..=100).text("CPS"));
                        if old_cps != cfg.cps {
                            changed = true;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.columns(2, |columns| {
                        columns[0].label(egui::RichText::new("Button").strong());
                        let old_btn = cfg.button;
                        columns[0].radio_value(&mut cfg.button, ClickButton::Left, "Left");
                        columns[0].radio_value(&mut cfg.button, ClickButton::Middle, "Middle");
                        columns[0].radio_value(&mut cfg.button, ClickButton::Right, "Right");
                        if old_btn != cfg.button {
                            changed = true;
                        }

                        columns[1].label(egui::RichText::new("Mode").strong());
                        let old_mode = cfg.mode;
                        columns[1].radio_value(&mut cfg.mode, ClickMode::Hold, "Hold");
                        columns[1].radio_value(&mut cfg.mode, ClickMode::Toggle, "Toggle");
                        if old_mode != cfg.mode {
                            self.is_clicking.store(false, Ordering::Relaxed);
                            changed = true;
                        }
                    });
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("Hotkeys").strong());
                    ui.add_space(4.0);
                    
                    egui::Grid::new("hotkeys_grid")
                        .num_columns(2)
                        .spacing([10.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Toggle:");
                            let old_toggle = cfg.toggle_key;
                            egui::ComboBox::from_id_source("toggle_key")
                                .selected_text(format!("{:?}", cfg.toggle_key))
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for key in [Code::F1, Code::F2, Code::F3, Code::F4, Code::F5, Code::F6, Code::F7, Code::F8, Code::F9, Code::F10, Code::F11, Code::F12, Code::Home, Code::End, Code::Insert, Code::Delete] {
                                        ui.selectable_value(&mut cfg.toggle_key, key, format!("{:?}", key));
                                    }
                                });
                            if old_toggle != cfg.toggle_key {
                                changed = true;
                            }
                            ui.end_row();

                            ui.label("Kill:");
                            let old_kill = cfg.kill_key;
                            egui::ComboBox::from_id_source("kill_key")
                                .selected_text(format!("{:?}", cfg.kill_key))
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for key in [Code::F1, Code::F2, Code::F3, Code::F4, Code::F5, Code::F6, Code::F7, Code::F8, Code::F9, Code::F10, Code::F11, Code::F12, Code::Home, Code::End, Code::Insert, Code::Delete] {
                                        ui.selectable_value(&mut cfg.kill_key, key, format!("{:?}", key));
                                    }
                                });
                            if old_kill != cfg.kill_key {
                                changed = true;
                            }
                            ui.end_row();
                        });
                });
            }

            if changed {
                self.update_hotkeys();
                self.save_config();
            }

            ui.add_space(16.0);

            let clicking = self.is_clicking.load(Ordering::Relaxed);
            let (status_text, status_color) = if clicking {
                ("RUNNING", egui::Color32::from_rgb(0, 255, 127))
            } else {
                ("IDLE", egui::Color32::from_rgb(150, 150, 150))
            };

            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 4.0);
                    ui.label("Status:");
                    ui.colored_label(status_color, egui::RichText::new(status_text).strong().size(18.0));
                });
                
                ui.add_space(8.0);
                ui.weak(format!("Press {:?} to toggle", self.config.read().unwrap().toggle_key));
            });
        });

        // Ensure we check for hotkeys frequently
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([280.0, 360.0])
            .with_min_inner_size([280.0, 360.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "AutoClickrs",
        options,
        Box::new(|cc| Box::new(AutoClickerApp::new(cc))),
    )
}
