mod store;

use crate::store::MailStore;
use anyhow::{anyhow, Context, Result};
use getopts::Options;
use log::error;
use mailin_embedded::response::{BAD_HELLO, BLOCKED_IP, INTERNAL_ERROR, OK};
use mailin_embedded::{Response, Server, SslConfig};
use mxdns::MxDns;
use simplelog::{
    ColorChoice, CombinedLogger, Config, LevelFilter, TermLogger, TerminalMode, WriteLogger,
};
use std::env;
use std::fs::File;
use std::io;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::path::Path;
use time::{format_description, OffsetDateTime};

const DOMAIN: &str = "localhost";
const DEFAULT_ADDRESS: &str = "127.0.0.1:8025";

// Command line option names
const OPT_HELP: &str = "help";
const OPT_ADDRESS: &str = "address";
const OPT_LOG: &str = "log";
const OPT_SERVER: &str = "server";
const OPT_SSL_CERT: &str = "ssl-cert";
const OPT_SSL_KEY: &str = "ssl-key";
const OPT_SSL_CHAIN: &str = "ssl-chain";
const OPT_BLOCKLIST: &str = "blocklist";
const OPT_MAILDIR: &str = "maildir";

#[derive(Clone)]
struct Handler<'a> {
    mxdns: &'a MxDns,
    mailstore: MailStore,
}

impl mailin_embedded::Handler for Handler<'_> {
    fn helo(&mut self, ip: IpAddr, _domain: &str) -> Response {
        if ip == Ipv4Addr::new(127, 0, 0, 1) {
            return OK;
        }
        // Does the reverse DNS match the forward dns?
        let rdns = self.mxdns.fcrdns(ip);
        match rdns {
            Ok(ref res) if !res.is_confirmed() => BAD_HELLO,
            _ => {
                if self.mxdns.is_blocked(ip).unwrap_or(false) {
                    BLOCKED_IP
                } else {
                    OK
                }
            }
        }
    }

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Response {
        match self.mailstore.start_message() {
            Ok(()) => OK,
            Err(err) => {
                error!("Start message: {}", err);
                INTERNAL_ERROR
            }
        }
    }

    fn data(&mut self, buf: &[u8]) -> io::Result<()> {
        self.mailstore.write_all(buf)
    }

    fn data_end(&mut self) -> Response {
        match self.mailstore.end_message() {
            Ok(()) => OK,
            Err(err) => {
                error!("End message: {}", err);
                INTERNAL_ERROR
            }
        }
    }
}

fn setup_logger(log_dir: Option<String>) -> Result<()> {
    let log_level = LevelFilter::Info;
    // Try to create a terminal logger, if this fails use a simple logger to stdout
    let term_logger = TermLogger::new(
        log_level,
        Config::default(),
        TerminalMode::Stdout,
        ColorChoice::Auto,
    );
    // Create a trace logger that writes SMTP interaction to file
    if let Some(dir) = log_dir {
        let log_path = Path::new(&dir);
        let filename = log_filename();
        let filepath = log_path.join(filename);
        let file = File::create(filepath)?;
        CombinedLogger::init(vec![
            term_logger,
            WriteLogger::new(LevelFilter::Trace, Config::default(), file),
        ])
        .context("Cannot initialize logger")
    } else {
        CombinedLogger::init(vec![term_logger]).context("Cannot initialize logger")
    }
}

fn log_filename() -> String {
    let datetime = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let date_suffix_format =
        format_description::parse("[year][month][day][hour][minute][second]").unwrap();
    let datetime = datetime
        .format(&date_suffix_format)
        .unwrap_or_else(|_| datetime.to_string());
    format!("smtp-{datetime}.log")
}

fn print_usage(program: &str, opts: &Options) {
    let brief = format!("Usage: {program} [options]");
    print!("{}", opts.usage(&brief));
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut opts = getopts::Options::new();
    opts.optflag("h", OPT_HELP, "print this help menu");
    opts.optopt("a", OPT_ADDRESS, "the address to listen on", "ADDRESS");
    opts.optopt("l", OPT_LOG, "the directory to write logs to", "LOG_DIR");
    opts.optopt("s", OPT_SERVER, "the name of the mailserver", "SERVER");
    opts.optmulti("", OPT_BLOCKLIST, "use blocklist", "BLOCKLIST");
    opts.optopt("", OPT_SSL_CERT, "ssl certificate", "PEM_FILE");
    opts.optopt("", OPT_SSL_KEY, "ssl certificate key", "PEM_FILE");
    opts.optopt(
        "",
        OPT_SSL_CHAIN,
        "ssl chain of trust for the certificate",
        "PEM_FILE",
    );
    opts.optopt("", OPT_MAILDIR, "the directory to store mail in", "MAILDIR");
    let matches = opts
        .parse(&args[1..])
        .context("Cannot parse command line")?;
    if matches.opt_present(OPT_HELP) {
        print_usage(&args[0], &opts);
        return Ok(());
    }
    let ssl_config = match (
        matches.opt_str(OPT_SSL_CERT),
        matches.opt_str(OPT_SSL_KEY),
        matches.opt_str(OPT_SSL_CHAIN),
    ) {
        (Some(cert_path), Some(key_path), Some(chain_path)) => SslConfig::Trusted {
            cert_path,
            key_path,
            chain_path,
        },
        (Some(cert_path), Some(key_path), None) => SslConfig::SelfSigned {
            cert_path,
            key_path,
        },
        (_, _, _) => SslConfig::None,
    };
    let domain = matches
        .opt_str(OPT_SERVER)
        .unwrap_or_else(|| DOMAIN.to_owned());
    let blocklists = matches.opt_strs(OPT_BLOCKLIST);
    let mxdns = MxDns::new(blocklists)?;
    let maildir = matches
        .opt_str(OPT_MAILDIR)
        .unwrap_or_else(|| "mail".to_owned());
    let handler = Handler {
        mxdns: &mxdns,
        mailstore: MailStore::new(maildir),
    };
    let mut server = Server::new(handler);
    server
        .with_name(domain)
        .with_ssl(ssl_config)
        .map_err(|e| anyhow!("Cannot initialise SSL: {}", e))?;
    // Bind TCP listener
    let addr = matches
        .opt_str(OPT_ADDRESS)
        .unwrap_or_else(|| DEFAULT_ADDRESS.to_owned());
    let listener = TcpListener::bind(addr)?;
    server.with_tcp_listener(listener);

    let log_directory = matches.opt_str(OPT_LOG);
    setup_logger(log_directory)?;

    server
        .serve()
        .map_err(|e| anyhow!("Cannot start server: {}", e))
}
