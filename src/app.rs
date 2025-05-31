use crate::parser::{self, LogEntry, LogLevel};
use async_std::task::current;
use chrono::{DateTime, Datelike, Utc};
use egui::{Color32, FontId, RichText, TextFormat, TextStyle, text::LayoutJob};
use egui_dock::{DockArea, DockState, Style};
use egui_modal::Modal;
use std::collections::BTreeMap;
use std::{
    io::Read,
    sync::{Arc, Mutex},
};
use strum::IntoEnumIterator;

struct TabContent {
    title: String,
    entries: parser::Entries,
    is_search: bool,
    filter: String,
    enabled_levels: Vec<LogLevel>,
    //TODO: Move this to reference
    filtered_entries: Vec<LogEntry>,
    heights: Vec<f32>,
    rx: regex::Regex,
}

impl TabContent {
    fn new(title: String, entries: parser::Entries) -> Self {
        Self {
            title,
            entries,
            is_search: true,
            filter: Default::default(),
            enabled_levels: LogLevel::iter()
                .filter(|x| *x != LogLevel::Unknown)
                .collect(),
            filtered_entries: Default::default(),
            heights: vec![],
            rx: regex::Regex::new("").unwrap(),
        }
    }
}

pub struct TemplateApp {
    worker: Arc<Mutex<parser::Worker>>,
    size_text: Arc<Mutex<String>>,
    open_model: bool,
    logs: parser::Processed,
    tree: DockState<TabContent>,
    tab_viewer: TabViewer,
    is_processing: bool,
    last_time: chrono::DateTime<chrono::Utc>,
    service_names: BTreeMap<String, bool>,
}

impl Default for TemplateApp {
    fn default() -> Self {
        Self {
            worker: Arc::new(Mutex::new(Default::default())),
            size_text: Default::default(),
            open_model: false,
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
            service_names: vec![
                "ardupilot-manager",
                "bag-of-holding",
                "beacon",
                "blueos_startup_update",
                "bootstrap",
                "bridget",
                "cable-guy",
                "commander",
                "helper",
                "kraken",
                "linux2rest",
                "log-zipper",
                "major_tom",
                "mavlink-camera-manager",
                "nmea-injector",
                "pardal",
                "ping",
                "telemetry",
                "version-chooser",
                "wifi-manage",
            ]
            .iter()
            .map(|&name| {
                let value = !name.contains("camera");
                (name.into(), value)
            })
            .collect(),
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
        let is_search = &mut tab.is_search;
        let filter = &mut tab.filter;
        let filtered_entries = &mut tab.filtered_entries;
        let rx = &mut tab.rx;
        let heights = &mut tab.heights;

        let text_height = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y);

        #[inline]
        fn reset_filter(
            entries: &parser::Entries,
            filtered_entries: &mut Vec<LogEntry>,
            heights: &mut Vec<f32>,
            text_height: f32,
        ) {
            *filtered_entries = entries.clone();
            *heights = filtered_entries
                .iter()
                .map(|entry| {
                    (entry.message.lines().count() as f32 * text_height * 0.9).max(text_height)
                })
                .collect();
        }
        if filter.is_empty() && filtered_entries.is_empty() {
            reset_filter(entries, filtered_entries, heights, text_height);
            filter.clear();
        }

        let mut current_is_search = is_search.clone();
        let mut current_row = None;
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if *is_search { "Search:" } else { "Filter:" })
                    .clicked()
                {
                    current_is_search = !*is_search;
                }
                let mut current_filter = filter.clone();
                let mut current_levels = tab.enabled_levels.clone();
                ui.add(egui::TextEdit::singleline(&mut current_filter).desired_width(120.0));
                if ui.button("ｘ").clicked() {
                    current_filter.clear();
                    filter.clear();
                    reset_filter(entries, filtered_entries, &mut tab.heights, text_height);
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

                ui.separator();
                if ui.button("Download").clicked() {
                    use std::io::Write;

                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("output.txt")
                        .save_file()
                    {
                        let mut file = std::fs::File::create(path).expect("Failed to create file");
                        file.write_all(entries_to_text(&entries).as_bytes())
                            .expect("Failed to write file");
                    }

                    #[cfg(target_arch = "wasm32")]
                    {
                        download_file(
                            "output.txt",
                            entries_to_text(&entries).as_bytes(),
                            "text/plain",
                        );
                    }
                }

                if *current_filter != *filter
                    || current_is_search != *is_search
                    || current_levels != tab.enabled_levels
                    || first_date != self.first_date
                    || second_date != self.second_date
                {
                    *is_search = current_is_search;
                    *filter = current_filter;
                    tab.enabled_levels = current_levels;
                    if let Ok(user_regex) = regex::RegexBuilder::new(filter)
                        .case_insensitive(true)
                        .build()
                    {
                        *rx = user_regex;
                        *filtered_entries = entries
                            .iter()
                            .filter(|entry| {
                                entry.timestamp.date_naive() > self.first_date
                                    && entry.timestamp.date_naive() < self.second_date
                            })
                            .filter(|entry| tab.enabled_levels.contains(&entry.level))
                            .filter(|entry| {
                                if *is_search {
                                    true
                                } else {
                                    rx.captures(&entry.message).is_some()
                                        || rx.captures(&entry.level.to_string()).is_some()
                                        || rx.captures(&entry.timestamp.to_string()).is_some()
                                }
                            })
                            .map(Clone::clone)
                            .collect();
                        tab.heights = filtered_entries
                            .iter()
                            .map(|entry| {
                                (entry.message.lines().count() as f32 * text_height * 0.9)
                                    .max(text_height)
                            })
                            .collect();
                    }

                    current_row = filtered_entries.iter().rposition(|entry| {
                        rx.captures(&entry.message).is_some()
                            || rx.captures(&entry.level.to_string()).is_some()
                            || rx.captures(&entry.timestamp.to_string()).is_some()
                    });
                }
            });

            use egui_extras::{Column, TableBuilder};

            let available_height = ui.available_height();
            let mut table = TableBuilder::new(ui)
                .striped(true)
                .auto_shrink(false)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::TOP))
                .column(Column::auto())
                .column(Column::auto())
                .column(Column::auto())
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height);

            table = table.sense(egui::Sense::click());
            if let Some(index) = current_row {
                table = table.scroll_to_row(index, Some(egui::Align::LEFT));
            }
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
                    body.heterogeneous_rows(tab.heights.iter().copied(), move |mut row| {
                        let row_index = row.index();
                        let entry = &filtered_entries[row_index];
                        row.col(|ui| {
                            if filter.is_empty() {
                                ui.label(&entry.timestamp.to_string());
                            } else {
                                let mut job = LayoutJob::default();
                                highlight_text_in_ui(&entry.timestamp.to_string(), rx, &mut job);
                                ui.label(job);
                            }
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
                            let mut job = LayoutJob::default();
                            if filter.is_empty() {
                                create_layout_from_terminal_escape_sequence(
                                    &entry.message,
                                    &mut job,
                                );
                            } else {
                                highlight_text_in_ui(entry.message.as_str(), rx, &mut job);
                            }
                            ui.label(job);
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

fn highlight_text_in_ui(message: &str, rx: &regex::Regex, job: &mut LayoutJob) {
    let mut last_end = 0;

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
}

fn create_layout_from_terminal_escape_sequence(input: &str, job: &mut LayoutJob) {
    let mut current_format = TextFormat::default();

    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            // Consume '['
            chars.next();

            // Collect the entire escape code
            let mut escape_code = String::new();
            while let Some(&num) = chars.peek() {
                if num.is_ascii_digit() || num == ';' {
                    escape_code.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            // Consume the final character ('m' in this case)
            let _ = chars.next();

            for code in escape_code.split(';') {
                match code {
                    "0" => current_format = TextFormat::default(), // Reset
                    "1" => current_format.underline = egui::Stroke::new(2.0, current_format.color), // Bold on (no bold in egui, use underline)
                    "3" => current_format.italics = true, // Italic on
                    "30" => current_format.color = Color32::BLACK,
                    "31" => current_format.color = Color32::RED,
                    "32" => current_format.color = Color32::GREEN,
                    "33" => current_format.color = Color32::YELLOW,
                    "34" => current_format.color = Color32::BLUE,
                    "35" => current_format.color = Color32::from_hex("#FF00FF").unwrap(),
                    "36" => current_format.color = Color32::from_hex("#00FFFF").unwrap(),
                    "37" => current_format.color = Color32::WHITE,
                    _ => (),
                }
            }
        } else {
            job.append(&c.to_string(), 0.0, current_format.clone());
        }
    }
}

impl TemplateApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }
}

impl eframe::App for TemplateApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let modal = Modal::new(ctx, "my_modal");
        let cloned_worker = self.worker.clone();

        // What goes inside the modal
        modal.show(|ui| {
            // these helper functions help set the ui based on the modal's
            // set style, but they are not required and you can put whatever
            // ui you want inside [`.show()`]
            modal.title(ui, "Open BlueOS Log file");
            modal.frame(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        for (service_name, checked) in &mut self.service_names {
                            ui.checkbox(checked, service_name);
                            ui.end_row();
                        }
                    });
                });
            });
            modal.buttons(ui, |ui| {
                // After clicking, the modal is automatically closed
                if modal.button(ui, "close").clicked() {
                    self.open_model = false;
                };

                if modal.button(ui, "Load file").clicked() {
                    let allowed_services: Vec<String> = self
                        .service_names
                        .iter()
                        .filter(|(_, value)| **value)
                        .map(|(name, _)| name)
                        .cloned()
                        .collect();
                    #[cfg(target_arch = "wasm32")]
                    {
                        let future = async move {
                            let file = rfd::AsyncFileDialog::new().pick_file().await;
                            let data = file.unwrap().read().await;
                            *cloned_worker.lock().unwrap() =
                                parser::process_from_zip(data, allowed_services);
                        };
                        async_std::task::block_on(future);
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        let data = std::fs::read(path).unwrap();
                        *cloned_worker.lock().unwrap() =
                            parser::process_from_zip(data, allowed_services);
                    }

                    self.open_model = false;
                }
            });
        });

        if self.open_model {
            modal.open();
        }
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            let now = chrono::prelude::Utc::now();
            let _delta = now - self.last_time;
            self.last_time = now;

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Select a file").clicked() {
                        self.open_model = true;

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

fn entries_to_text(entries: &parser::Entries) -> String {
    entries
        .iter()
        .map(|entry| format!("{}\t{}\t{}", entry.timestamp, entry.level, entry.message))
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(target_arch = "wasm32")]
use web_sys::wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use web_sys::wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url, window};

#[cfg(target_arch = "wasm32")]
fn download_file(filename: &str, content: &[u8], mime_type: &str) {
    let array = js_sys::Array::new();
    array.push(&JsValue::from_str(std::str::from_utf8(&content).unwrap()));

    let mut blob_options = BlobPropertyBag::new();
    blob_options.type_(mime_type);

    let blob = Blob::new_with_u8_array_sequence_and_options(&array.into(), &blob_options)
        .expect("Failed to create Blob");

    let url = Url::create_object_url_with_blob(&blob).expect("Failed to create URL");

    let document = window().unwrap().document().unwrap();
    let a = document
        .create_element("a")
        .unwrap()
        .dyn_into::<HtmlAnchorElement>()
        .unwrap();
    a.set_href(&url);
    a.set_download(filename);
    a.style().set_property("display", "none").unwrap();

    document.body().unwrap().append_child(&a).unwrap();
    a.click();
    document.body().unwrap().remove_child(&a).unwrap();

    Url::revoke_object_url(&url).expect("Failed to revoke URL");
}
