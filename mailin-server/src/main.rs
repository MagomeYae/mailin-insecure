use chrono::Local;
use failure::{format_err, Error};
use getopts::Options;
use mailin_embedded::{HeloResult, Server, SslConfig};
use mxdns::MxDns;
use nix::unistd;
use privdrop::PrivDrop;
use simplelog::{
    CombinedLogger, Config, LevelFilter, SharedLogger, SimpleLogger, TermLogger, TerminalMode,
    WriteLogger,
};
use std::env;
use std::fs::File;
use std::net::{IpAddr, TcpListener};
use std::path::Path;

const DOMAIN: &str = "localhost";
const DEFAULT_ADDRESS: &str = "127.0.0.1:8025";
const DEFAULT_USER: &str = "mailin";

// Command line option names
const OPT_HELP: &str = "help";
const OPT_ADDRESS: &str = "address";
const OPT_LOG: &str = "log";
const OPT_SERVER: &str = "server";
const OPT_SSL_CERT: &str = "ssl-cert";
const OPT_SSL_KEY: &str = "ssl-key";
const OPT_SSL_CHAIN: &str = "ssl-chain";
const OPT_BLOCKLIST: &str = "blocklist";
const OPT_USER: &str = "user";
const OPT_GROUP: &str = "group";

#[derive(Clone)]
struct Handler {
    mxdns: MxDns,
}

impl mailin_embedded::Handler for Handler {
    fn helo(&mut self, ip: IpAddr, _domain: &str) -> HeloResult {
        // Does the reverse DNS match the forward dns?
        let rdns = self.mxdns.fcrdns(ip);
        match rdns {
            Ok(ref res) if !res.is_confirmed() => HeloResult::BadHelo,
            _ => {
                if self.mxdns.is_blocked(ip).unwrap_or(false) {
                    HeloResult::BlockedIp
                } else {
                    HeloResult::Ok
                }
            }
        }
    }
}

fn setup_logger(log_dir: Option<String>) -> Result<(), Error> {
    let log_level = LevelFilter::Info;
    // Try to create a terminal logger, if this fails use a simple logger to stdout
    let term_logger = TermLogger::new(log_level, Config::default(), TerminalMode::Stdout);
    let quiet_logger: Box<dyn SharedLogger> = match term_logger {
        Some(tlog) => tlog,
        None => SimpleLogger::new(log_level, Config::default()),
    };
    // Create a trace logger that writes SMTP interaction to file
    if let Some(dir) = log_dir {
        let log_path = Path::new(&dir);
        let datetime = Local::now().format("%Y%m%d%H%M%S").to_string();
        let filename = format!("smtp-{}.log", datetime);
        let filepath = log_path.join(&filename);
        let file = File::create(&filepath)?;
        CombinedLogger::init(vec![
            quiet_logger,
            WriteLogger::new(LevelFilter::Trace, Config::default(), file),
        ])
        .map_err(|err| format_err!("Cannot initialize logger: {}", err))
    } else {
        CombinedLogger::init(vec![quiet_logger])
            .map_err(|err| format_err!("Cannot initialize logger: {}", err))
    }
}

fn print_usage(program: &str, opts: &Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() -> Result<(), Error> {
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
    opts.optopt("", OPT_USER, "user to run as", "USER");
    opts.optopt("", OPT_GROUP, "group to run as", "GROUP");
    let matches = opts
        .parse(&args[1..])
        .map_err(|err| format_err!("Error parsing command line: {}", err))?;
    if matches.opt_present(OPT_HELP) {
        print_usage(&args[0], &opts);
        return Ok(());
    }
    let ssl_config = match (matches.opt_str(OPT_SSL_CERT), matches.opt_str(OPT_SSL_KEY)) {
        (Some(cert_path), Some(key_path)) => SslConfig::SelfSigned {
            cert_path,
            key_path,
        },
        (_, _) => SslConfig::None,
    };
    let domain = matches
        .opt_str(OPT_SERVER)
        .unwrap_or_else(|| DOMAIN.to_owned());
    let blocklists = matches.opt_strs(OPT_BLOCKLIST);
    let mxdns = MxDns::new(blocklists).map_err(|e| format_err!("{}", e))?;
    let handler = Handler { mxdns };
    let mut server = Server::new(handler);
    server
        .with_name(domain)
        .with_ssl(ssl_config)
        .map_err(|e| format_err!("{}", e))?;

    // Bind TCP listener
    let addr = matches
        .opt_str(OPT_ADDRESS)
        .unwrap_or_else(|| DEFAULT_ADDRESS.to_owned());
    let listener = TcpListener::bind(addr)?;
    server.with_tcp_listener(listener);

    // Drop privileges if root
    if unistd::geteuid().is_root() {
        let user = matches
            .opt_str(OPT_USER)
            .unwrap_or_else(|| DEFAULT_USER.to_owned());
        let mut privdrop = PrivDrop::default().user(user);
        if let Some(group) = matches.opt_str(OPT_GROUP) {
            privdrop = privdrop.group(group);
        }
        privdrop.apply()?;
    }

    let log_directory = matches.opt_str(OPT_LOG);
    setup_logger(log_directory)?;

    server.serve_forever().map_err(|e| format_err!("{}", e))
}
