use log::{error, info};
use mailin_embedded::response::{INTERNAL_ERROR, TRANSACTION_FAILED};
use mailin_embedded::{Data, Response};
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

#[derive(Clone)]
pub struct MailStore {
    dir: PathBuf,
    counter: Arc<AtomicU32>,
}

pub struct State {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl MailStore {
    pub fn new<P>(dir: P) -> Self
    where
        P: Into<PathBuf> + Debug,
    {
        Self {
            dir: dir.into(),
            counter: Arc::new(AtomicU32::new(0)),
        }
    }

    fn start_message(&mut self) -> io::Result<State> {
        let mut path = self.dir.clone();
        path.push("tmp");
        fs::create_dir_all(&path)?;
        let message_file = self.message_file();
        path.push(message_file);
        info!("Writing message to {:#?}", path);
        let file = File::create(&path)?;
        let writer = BufWriter::new(file);
        Ok(State { path, writer })
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
}

impl Data for MailStore {
    type State = State;
    type Output = ();

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Result<Self::State, Response> {
        self.start_message().map_err(|err| {
            error!("Start message: {}", err);
            INTERNAL_ERROR
        })
    }

    fn data(&mut self, state: &mut State, buf: &[u8]) -> Result<(), Response> {
        state.writer.write_all(buf).map_err(|err| {
            error!("Error saving message: {}", err);
            TRANSACTION_FAILED
        })
    }

    fn data_end(&mut self, state: State) -> Result<(), Response> {
        commit_message(&state.path).map_err(|err| {
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
