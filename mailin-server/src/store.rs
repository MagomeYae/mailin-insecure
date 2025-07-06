use log::{error, info};
use mailin_embedded::response::{INTERNAL_ERROR, TRANSACTION_FAILED};
use mailin_embedded::Response;
use mime_event::MessageParser;
use std::fmt::Debug;
use std::fs;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

pub struct MailStore {
    dir: PathBuf,
    counter: Arc<AtomicU32>,
    state: Option<State>,
}

struct State {
    path: PathBuf,
    parser: MessageParser<BufWriter<File>>,
}

impl Clone for MailStore {
    fn clone(&self) -> Self {
        Self {
            dir: self.dir.clone(),
            counter: self.counter.clone(),
            state: None,
        }
    }
}

impl MailStore {
    pub fn new<P>(dir: P) -> Self
    where
        P: Into<PathBuf> + Debug,
    {
        Self {
            dir: dir.into(),
            counter: Arc::new(AtomicU32::new(0)),
            state: None,
        }
    }

    fn start_message(&mut self) -> io::Result<()> {
        let mut path = self.dir.clone();
        path.push("tmp");
        fs::create_dir_all(&path)?;
        let message_file = self.message_file();
        path.push(message_file);
        info!("Writing message to {:#?}", path);
        let file = File::create(&path)?;
        let writer = BufWriter::new(file);
        self.state.replace(State {
            path,
            parser: MessageParser::new(writer),
        });
        Ok(())
    }

    fn end_message(&mut self) -> io::Result<()> {
        self.state
            .take()
            .map(|state| {
                let message = state.parser.end();
                info!("{:#?}", message);
                commit_message(&state.path)
            })
            .unwrap_or(Ok(()))
    }

    fn message_file(&self) -> String {
        let mut filename = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis().to_string())
            .unwrap_or_else(|_| "0000".to_string());
        filename.push('.');
        filename.push_str(&process::id().to_string());
        filename.push('.');
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        filename.push_str(&count.to_string());
        filename
    }

    pub fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> anyhow::Result<(), Response> {
        self.start_message().map_err(|err| {
            error!("Start message: {}", err);
            INTERNAL_ERROR
        })
    }

    pub fn data(&mut self, buf: &[u8]) -> Result<(), Response> {
        self.state
            .as_mut()
            .map(|state| {
                state.parser.write_all(buf).map_err(|err| {
                    error!("Error saving message: {}", err);
                    TRANSACTION_FAILED
                })
            })
            .unwrap_or_else(|| Ok(()))
    }

    pub fn data_end(&mut self) -> Result<(), Response> {
        self.end_message().map_err(|err| {
            error!("End message: {}", err);
            INTERNAL_ERROR
        })
    }
}

fn commit_message(tmp_path: &Path) -> io::Result<()> {
    let filename = tmp_path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let mut dest = tmp_path.to_path_buf();
    dest.pop();
    dest.pop();
    dest.push("new");
    fs::create_dir_all(&dest)?;
    dest.push(filename);
    fs::rename(tmp_path, dest)
}
