use chrono::{DateTime, Utc};
use dateparser;
use flate2::read::GzDecoder;
use regex::Regex;
use std::sync::{Arc, Mutex};
use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek},
};
use zip::ZipArchive;

use std::sync::Once;

static mut REGEX_GENERAL: Option<Regex> = None;
static mut REGEX_DETAILED: Option<Regex> = None;
static INIT: Once = Once::new();

pub type LogBook = BTreeMap<String, Vec<LogEntry>>;
pub type Entries = Vec<LogEntry>;

#[derive(Clone, Debug)]
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

    pub fn to_string(&self) -> String {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
            _ => "UNKNOWN",
        }
        .to_string()
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
        let (regex_general, regex_detailed) = unsafe {
            INIT.call_once(|| {
                REGEX_GENERAL = Some(Regex::new(
                    concat!(
                            r"^(?P<timestamp>\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}\.\d{3,6}Z?)\s*\|\s*",
                            r"(?P<level>\S+)\s*\|\s*",
                            // r"(?P<component>[\w-]+(?:[:]\w+)?[:]\w+[:]\d+)\s*-\s*",
                            r"(?P<message>.+)$",
                        )
                    ).unwrap());

                REGEX_DETAILED = Some(Regex::new(
                    concat!(
                            r"^(?P<timestamp>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{6}Z)\s+",
                            r"(?P<level>\S+)\s+",
                            // r"(?P<component>[^\s]+)\s+ThreadId\(\d+\)\s+",
                            r"(?P<message>.+)$",
                        )
                    ).unwrap());
            });
            (
                REGEX_GENERAL.as_ref().unwrap(),
                REGEX_DETAILED.as_ref().unwrap(),
            )
        };

        regex_general
            .captures(line)
            .or_else(|| regex_detailed.captures(line))
            .and_then(|caps| {
                let timestamp = dateparser::parse(&caps["timestamp"])
                    .ok()
                    .expect("Failed to parse timestamp");
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
        match *state {
            ProcessingState::None => {
                *state = ProcessingState::Processing(Info {
                    service_name: file.split("/").next().unwrap().into(),
                    percentage: 0.0,
                    file,
                });
            }
            _ => {}
        }
    }
}

pub fn process_from_zip(data: Vec<u8>) -> Worker {
    let worker = Worker::default();
    let cloned_worker = worker.clone();

    use tokio_with_wasm::tokio_wasm as tokio;

    tokio::spawn(async move {
        let started = chrono::prelude::Utc::now();
        let reader = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(reader).unwrap();
        let mut logs: LogBook = BTreeMap::new();

        log::info!("Started processing {:#?}", chrono::prelude::Utc::now());
        let size = archive.len();
        let mut file_size = 0;
        for i in 0..size {
            let mut file = archive.by_index(i).unwrap();
            let service_name: String = file.name().split("/").next().unwrap().into();
            let file_name = file.name().to_string();
            log::info!("Processing: {}", file_name);
            if file.is_file()
                && (file.name().ends_with(".gz")
                    || file.name().ends_with(".zip")
                    || file.name().ends_with(".log"))
            {
                file_size += file.size() as usize;
                let buf_reader = std::io::BufReader::new(flate2::read::GzDecoder::new(&mut file));
                let mut entries = process_log_file(buf_reader).unwrap();
                if let Some(service) = logs.get_mut(&service_name) {
                    service.append(&mut entries);
                } else {
                    logs.insert(service_name.clone(), entries);
                }
            }
            *cloned_worker.state.lock().unwrap() = ProcessingState::Processing(Info {
                service_name,
                percentage: 100.0 * i as f64 / size as f64,
                file: file.name().to_string(),
            });
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        log::info!("Done with processing {:#?}", chrono::prelude::Utc::now());

        let keys: Vec<String> = logs.keys().into_iter().map(|value| value.clone()).collect();
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

pub fn process_log_file<R: Read>(reader: BufReader<R>) -> io::Result<Vec<LogEntry>> {
    let lines: Vec<String> = reader.lines().filter_map(|line| line.ok()).collect();

    let mut entries = vec![];
    for line in lines {
        if let Some(entry) = LogEntry::parse(&line) {
            entries.push(entry);
            continue;
        }

        let Some(last_entry) = entries.last_mut() else {
            continue;
        };

        last_entry.message.push_str(&line);
    }

    Ok(entries)
}
