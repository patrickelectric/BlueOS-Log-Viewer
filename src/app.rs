use crate::parser::{self, LogEntry, LogLevel};
use chrono::{DateTime, Datelike, Utc};
use egui::{text::LayoutJob, Color32, FontId, RichText, TextFormat, TextStyle};
use egui_dock::{DockArea, DockState, Style};
use std::{
    io::Read,
    sync::{Arc, Mutex},
};
use strum::IntoEnumIterator;

struct TabContent {
    title: String,
    entries: parser::Entries,
    filter: String,
    enabled_levels: Vec<LogLevel>,
    //TODO: Move this to reference
    filtered_entries: Vec<LogEntry>,
    rx: regex::Regex,
}

impl TabContent {
    fn new(title: String, entries: parser::Entries) -> Self {
        Self {
            title,
            entries,
            filter: Default::default(),
            enabled_levels: LogLevel::iter()
                .filter(|x| *x != LogLevel::Unknown)
                .collect(),
            filtered_entries: Default::default(),
            rx: regex::Regex::new("").unwrap(),
        }
    }
}

pub struct TemplateApp {
    worker: Arc<Mutex<parser::Worker>>,
    size_text: Arc<Mutex<String>>,
    logs: parser::Processed,
    tree: DockState<TabContent>,
    tab_viewer: TabViewer,
    is_processing: bool,
    last_time: chrono::DateTime<chrono::Utc>,
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            worker: Arc::new(Mutex::new(Default::default())),
            size_text: Default::default(),
            logs: Default::default(),
            tree: DockState::new(vec![]),
            tab_viewer: TabViewer {
                selected_date: None,
                first_date: chrono::offset::Utc::now()
                    .with_year(2020)
                    .unwrap()
                    .date_naive(),
                second_date: (chrono::offset::Utc::now() + chrono::Months::new(1)).date_naive(),
            },
            is_processing: false,
            last_time: chrono::prelude::Utc::now(),
        }
    }
}

struct TabViewer {
    selected_date: Option<DateTime<Utc>>,
    first_date: chrono::NaiveDate,
    second_date: chrono::NaiveDate,
}

impl egui_dock::TabViewer for TabViewer {
    type Tab = TabContent;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title.clone().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        let entries = &tab.entries;
        let filter = &mut tab.filter;
        let filtered_entries = &mut tab.filtered_entries;
        let rx = &mut tab.rx;

        if filter.is_empty() && filtered_entries.is_empty() {
            *filtered_entries = entries.clone();
        }

        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");
                let mut current_filter = filter.clone();
                let mut current_levels = tab.enabled_levels.clone();
                ui.add(egui::TextEdit::singleline(&mut current_filter).desired_width(120.0));
                if ui.button("ｘ").clicked() {
                    current_filter.clear();
                }

                ui.separator();
                ui.label("Levels:");
                for log_enum in LogLevel::iter() {
                    if log_enum == LogLevel::Unknown {
                        continue;
                    }
                    let mut enabled = current_levels.contains(&log_enum);
                    if ui
                        .add(egui::Checkbox::new(&mut enabled, log_enum.to_string()))
                        .changed()
                    {
                        if enabled {
                            current_levels.push(log_enum);
                        } else {
                            current_levels.retain(|x| *x != log_enum);
                        }
                    }
                }

                ui.separator();
                ui.label("Date range:");
                let first_date = self.first_date.clone();
                let second_date = self.second_date.clone();
                ui.add(egui_extras::DatePickerButton::new(&mut self.first_date).id_source("First"));
                ui.add(
                    egui_extras::DatePickerButton::new(&mut self.second_date).id_source("Second"),
                );

                if *current_filter != *filter
                    || current_levels != tab.enabled_levels
                    || first_date != self.first_date
                    || second_date != self.second_date
                {
                    tab.enabled_levels = current_levels;
                    *filter = current_filter;
                    if let Ok(rx) = regex::RegexBuilder::new(filter)
                        .case_insensitive(true)
                        .build()
                    {
                        *filtered_entries = entries
                            .iter()
                            .filter(|entry| {
                                entry.timestamp.date_naive() > self.first_date
                                    && entry.timestamp.date_naive() < self.second_date
                            })
                            .filter(|entry| tab.enabled_levels.contains(&entry.level))
                            .filter(|entry| {
                                rx.captures(&entry.message).is_some()
                                    || rx.captures(&entry.level.to_string()).is_some()
                                    || rx.captures(&entry.timestamp.to_string()).is_some()
                            })
                            .map(Clone::clone)
                            .collect();
                    }
                }
            });

            use egui_extras::{Column, TableBuilder};

            let text_height = egui::TextStyle::Body
                .resolve(ui.style())
                .size
                .max(ui.spacing().interact_size.y);

            let available_height = ui.available_height();
            let mut table = TableBuilder::new(ui)
                .striped(true)
                .auto_shrink(false)
                .resizable(true)
                .stick_to_bottom(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto())
                .column(Column::auto())
                .column(Column::auto())
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height);

            table = table.sense(egui::Sense::click());

            table
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Timestamp");
                    });
                    header.col(|ui| {
                        ui.strong("Level");
                    });
                    header.col(|ui| {
                        ui.strong("Content");
                    });
                })
                .body(|body| {
                    let size = filtered_entries.len();
                    body.rows(text_height, size, move |mut row| {
                        let row_index = row.index();
                        let entry = &filtered_entries[row_index];
                        row.col(|ui| {
                            ui.label(entry.timestamp.to_string());
                        });
                        row.col(|ui| {
                            let color = match entry.level {
                                parser::LogLevel::Error => Color32::from_hex("#D55E00").unwrap(),
                                parser::LogLevel::Warn => Color32::from_hex("#E69F00").unwrap(),
                                parser::LogLevel::Info => Color32::from_hex("#56B4E9").unwrap(),
                                parser::LogLevel::Debug => Color32::from_hex("#CC79A7").unwrap(),
                                parser::LogLevel::Trace => Color32::GRAY,
                                parser::LogLevel::Unknown => Color32::GOLD,
                            };
                            ui.label(RichText::new(entry.level.to_string()).color(color));
                        });
                        row.col(|ui| {
                            if filter.is_empty() {
                                ui.label(entry.message.to_string());
                            } else {
                                highlight_text_in_ui(ui, entry.message.as_str(), rx);
                            }
                        });

                        if row.response().clicked() {
                            dbg!(&entry.timestamp);
                            self.selected_date = Some(entry.timestamp);
                        }
                    });
                });
        });
    }
}

fn highlight_text_in_ui(ui: &mut egui::Ui, message: &str, rx: &regex::Regex) {
    let mut last_end = 0;

    let mut job = LayoutJob::default();

    // Iterate over all matches in the message
    for mat in rx.find_iter(message) {
        if last_end != mat.start() {
            // Add non-matching text with default formatting
            job.append(
                &message[last_end..mat.start()],
                0.0,
                TextFormat {
                    ..Default::default()
                },
            );
        }
        // Add matching text with highlighted formatting
        job.append(
            &message[mat.start()..mat.end()],
            0.0,
            TextFormat {
                color: Color32::BLACK,
                background: Color32::from_hex("#E69F00").unwrap(),
                ..Default::default()
            },
        );
        last_end = mat.end();
    }

    if last_end < message.len() {
        // Add remaining non-matching text
        job.append(
            &message[last_end..],
            0.0,
            TextFormat {
                ..Default::default()
            },
        );
    }

    // Display the formatted text as a label
    ui.label(job);
}

impl TemplateApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }
}

impl eframe::App for TemplateApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            let now = chrono::prelude::Utc::now();
            let _delta = now - self.last_time;
            self.last_time = now;

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Select a file").clicked() {
                        let cloned_worker = self.worker.clone();

                        #[cfg(target_arch = "wasm32")]
                        {
                            let future = async move {
                                let file = rfd::AsyncFileDialog::new().pick_file().await;
                                let data = file.unwrap().read().await;
                                *cloned_worker.lock().unwrap() = parser::process_from_zip(data);
                            };
                            async_std::task::block_on(future);
                        }

                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            let data = std::fs::read(path).unwrap();
                            *cloned_worker.lock().unwrap() = parser::process_from_zip(data);
                        }

                        ui.close_menu();
                        self.logs = Default::default();
                        self.is_processing = true;
                    }

                    let is_web = cfg!(target_arch = "wasm32");
                    if !is_web && ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(16.0);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
                    egui::widgets::global_dark_light_mode_switch(ui);
                    ui.separator();
                    if self.is_processing {
                        if let Some(info) = self.worker.lock().unwrap().info() {
                            ui.label(format!(
                                "Processing: {}   {:.2}% {}",
                                info.file,
                                info.percentage,
                                &bytesize::ByteSize(info.size as u64).to_string(),
                            ));
                        }
                    } else {
                        let size = self.logs.size;
                        ui.label(format!(
                            "{} [{}]",
                            &bytesize::ByteSize(size as u64).to_string(),
                            humantime::format_duration(self.logs.duration.to_std().unwrap())
                                .to_string()
                        ));
                    }
                });
            });
        });

        egui::SidePanel::right("egui_demo_panel")
            .resizable(false)
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Services");
                });

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        if self.logs.logbook.is_empty() {
                            if let Some(p) = self.worker.lock().unwrap().processed() {
                                self.logs = p;
                                self.is_processing = false;
                            }
                        }

                        self.logs.logbook.iter().for_each(|(service, entries)| {
                            if ui.button(service).clicked() {
                                if self.tree.main_surface().num_tabs() == 0 {
                                    let tab = TabContent::new(service.clone(), entries.clone());
                                    self.tree.main_surface_mut().push_to_first_leaf(tab);
                                } else {
                                    let mut tab_name = service.clone();

                                    while self
                                        .tree
                                        .iter_all_tabs()
                                        .any(|(_data, tab)| *tab.title == tab_name)
                                    {
                                        tab_name += "+"
                                    }
                                    let tab = TabContent::new(tab_name, entries.clone());
                                    self.tree.add_window(vec![tab]);
                                }
                            }
                        });
                    });
                });

                ui.separator();
            });

        egui::CentralPanel::default().show(ctx, |_ui| {
            DockArea::new(&mut self.tree)
                .style(Style::from_egui(ctx.style().as_ref()))
                .show(ctx, &mut self.tab_viewer);
        });

        if self.is_processing {
            ctx.request_repaint();
        }
    }
}
