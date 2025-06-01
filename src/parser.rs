use chrono::{DateTime, Utc};
use dateparser;
use flate2::read::GzDecoder;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::{Arc, Mutex};
use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek},
};
use strum_macros::EnumIter;
use zip::ZipArchive;

#[cfg(target_arch = "wasm32")]
use tokio_with_wasm::tokio_wasm as tokio;

#[cfg(not(target_arch = "wasm32"))]
use tokio;

static REGEX_GENERAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"^(?P<timestamp>\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}\.\d{3,6}Z?)\s*\|\s*",
        r"(?P<level>\S+)\s*\|\s*",
        // r"(?P<component>[\w-]+(?:[:]\w+)?[:]\w+[:]\d+)\s*-\s*",
        r"(?P<message>.+)$",
    ))
    .unwrap()
});

static REGEX_DETAILED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"^(?P<timestamp>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{6}Z)\s+",
        r"(?P<level>\S+)\s+",
        // r"(?P<component>[^\s]+)\s+ThreadId\(\d+\)\s+",
        r"(?P<message>.+)$",
    ))
    .unwrap()
});

pub type LogBook = BTreeMap<String, Vec<LogEntry>>;
pub type Entries = Vec<LogEntry>;

#[derive(Clone, Debug, EnumIter, PartialEq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
    Unknown,
}

impl LogLevel {
    fn from_str(s: &str) -> Self {
        match s.trim() {
            "ERROR" => LogLevel::Error,
            "WARN" | "WARNING" => LogLevel::Warn,
            "INFO" => LogLevel::Info,
            "DEBUG" => LogLevel::Debug,
            "TRACE" => LogLevel::Trace,
            _ => LogLevel::Unknown,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Trace => write!(f, "TRACE"),
            _ => write!(f, "UNKNOWN"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub component: Option<String>,
    pub message: String,
}

impl LogEntry {
    fn parse(line: &str) -> Option<Self> {
        REGEX_GENERAL
            .captures(line)
            .or_else(|| REGEX_DETAILED.captures(line))
            .and_then(|caps| {
                let Ok(timestamp) =
                    dateparser::parse_with_timezone(&caps["timestamp"], &chrono::Utc)
                else {
                    log::error!("Failed to parse timestamp");
                    return None;
                };
                let level = LogLevel::from_str(&caps["level"]);
                let message = caps["message"].to_string();
                Some(LogEntry {
                    timestamp,
                    level,
                    component: None,
                    message,
                })
            })
    }
}

#[derive(Clone)]
pub struct Info {
    pub service_name: String,
    pub percentage: f64,
    pub size: usize,
    pub file: String,
}

#[derive(Clone, Default)]
pub struct Processed {
    pub logbook: LogBook,
    pub size: usize,
    pub duration: chrono::TimeDelta,
}

#[derive(Clone)]
enum ProcessingState {
    Done(Processed),
    Processing(Info),
    None,
}

#[derive(Clone)]
pub struct Worker {
    state: Arc<Mutex<ProcessingState>>,
}

impl Default for Worker {
    fn default() -> Self {
        Worker {
            state: Arc::new(Mutex::new(ProcessingState::None)),
        }
    }
}

impl Worker {
    pub fn logs(&self) -> Option<LogBook> {
        if let ProcessingState::Done(p) = &*self.state.lock().unwrap() {
            return Some(p.logbook.clone());
        }
        None
    }

    pub fn processed(&self) -> Option<Processed> {
        if let ProcessingState::Done(p) = &*self.state.lock().unwrap() {
            return Some(p.clone());
        }
        None
    }

    pub fn info(&self) -> Option<Info> {
        if let ProcessingState::Processing(info) = &*self.state.lock().unwrap() {
            return Some(info.clone());
        }
        None
    }
}

impl Worker {
    fn process(&self, file: String) {
        let mut state = self.state.lock().unwrap();
        if let ProcessingState::None = *state {
            *state = ProcessingState::Processing(Info {
                service_name: file.split('/').next().unwrap().into(),
                percentage: 0.0,
                size: 0,
                file,
            });
        }
    }
}

fn get_service_name(file: &str) -> String {
    let names = file.split('/').collect::<Vec<&str>>();
    let service_name = if names.len() > 1 {
        names[names.len() - 2].to_string()
    } else {
        names[0].to_string()
    };
    service_name
}

pub fn process_from_zip(data: Vec<u8>, allowed_versions: Vec<String>) -> Worker {
    let worker = Worker::default();
    let cloned_worker = worker.clone();

    tokio::spawn(async move {
        let started = chrono::prelude::Utc::now();
        let reader = std::io::Cursor::new(data);
        let mut archive = ZipArchive::new(reader).unwrap();
        let mut logs: LogBook = BTreeMap::new();
        log::info!("Started processing {:#?}", chrono::prelude::Utc::now());
        let size = archive.len();
        let mut file_size = 0;
        for i in 0..size {
            let mut file = archive.by_index(i).unwrap();
            if !file.is_file() {
                continue;
            }
            let file_name = file.name().to_string();
            let service_name = get_service_name(&file_name);
            if !allowed_versions.contains(&service_name) {
                continue;
            }

            if file.size() > 0 {
                let processed = if file.name().ends_with(".gz") {
                    process_log_file(std::io::BufReader::new(GzDecoder::new(&mut file)))
                } else if file.name().ends_with(".log") {
                    process_log_file(std::io::BufReader::new(&mut file))
                } else if file.name().ends_with(".zip") {
                    let mut inner_data = Vec::new();
                    file.read_to_end(&mut inner_data).unwrap();
                    // log::info!("Content: {:?}", &inner_data[0..10]);
                    let mut archive = match ZipArchive::new(std::io::Cursor::new(inner_data)) {
                        Ok(archive) => archive,
                        Err(e) => {
                            log::error!("Failed to open inner zip: {} {:#?}", &file_name, e);
                            continue;
                        }
                    };
                    let size_u = archive.len();
                    for u in 0..size_u {
                        let mut file = archive.by_index(u).unwrap();
                        let file_name = file.name().to_string();
                        let service_name = get_service_name(&file_name);
                        if !allowed_versions.contains(&service_name) {
                            continue;
                        }
                        // log::info!("Processing zip: {}", file_name);
                        if !file.is_file() {
                            continue;
                        }
                        if file.size() > 0 {
                            let processed = if file.name().ends_with(".gz") {
                                process_log_file(std::io::BufReader::new(GzDecoder::new(&mut file)))
                            } else if file.name().ends_with(".log") {
                                process_log_file(std::io::BufReader::new(&mut file))
                            } else {
                                continue;
                            };
                            let (mut entries, processed_size) = processed.unwrap();
                            file_size += processed_size;
                            if let Some(service) = logs.get_mut(&service_name) {
                                service.append(&mut entries);
                            } else {
                                logs.insert(service_name.clone(), entries);
                            }
                        } else {
                            continue;
                        };

                        *cloned_worker.state.lock().unwrap() = ProcessingState::Processing(Info {
                            service_name,
                            percentage: 100.0 * (i as f64 + u as f64 / size_u as f64) / size as f64,
                            size: file_size,
                            file: file.name().to_string(),
                        });

                        // Allow frontend to render
                        #[cfg(target_arch = "wasm32")]
                        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    }
                    continue;
                } else {
                    continue;
                };

                let (mut entries, processed_size) = processed.unwrap();
                file_size += processed_size;
                if let Some(service) = logs.get_mut(&service_name) {
                    service.append(&mut entries);
                } else {
                    logs.insert(service_name.clone(), entries);
                }
            }

            *cloned_worker.state.lock().unwrap() = ProcessingState::Processing(Info {
                service_name,
                percentage: 100.0 * i as f64 / size as f64,
                size: file_size,
                file: file.name().to_string(),
            });

            // Allow frontend to render
            #[cfg(target_arch = "wasm32")]
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        log::info!("Done with processing {:#?}", chrono::prelude::Utc::now());

        let keys: Vec<String> = logs.keys().cloned().collect();
        for key in keys {
            logs.get_mut(&key)
                .unwrap()
                .sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        }
        *cloned_worker.state.lock().unwrap() = ProcessingState::Done(Processed {
            logbook: logs,
            size: file_size,
            duration: chrono::prelude::Utc::now() - started,
        });
    });

    worker
}

pub fn process_log_file<R: Read>(reader: BufReader<R>) -> io::Result<(Vec<LogEntry>, usize)> {
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let mut size = 0;
    let mut entries = vec![];
    for line in lines {
        size += line.len();
        let line = line.trim_end_matches(['\0']);
        if let Some(entry) = LogEntry::parse(&line) {
            entries.push(entry);
            continue;
        }

        let Some(last_entry) = entries.last_mut() else {
            continue;
        };

        last_entry.message.push_str(&format!("\n{}", &line));
    }

    Ok((entries, size))
}
