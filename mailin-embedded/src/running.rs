use crate::err::Error;
use crate::Server;
use bufstream::BufStream;
use lazy_static::lazy_static;
use log::{debug, error, info, trace};
use mailin::{Action, Handler, Response, Session, SessionBuilder};
use openssl;
use openssl::ssl::{SslAcceptor, SslStream};
use scoped_threadpool::Pool;
use std::io::{BufRead, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

lazy_static! {
    static ref FIVE_MINUTES: Duration = Duration::new(5 * 60, 0);
}

enum SessionResult {
    Finished,
    UpgradeTls,
}

struct ServerState<H>
where
    H: Handler + Clone + Send,
{
    listener: TcpListener,
    handler: H,
    session_builder: SessionBuilder,
    ssl_acceptor: Option<Arc<SslAcceptor>>,
    num_threads: u32,
}

pub(crate) fn serve<H>(config: Server<H>) -> Result<(), Error>
where
    H: Handler + Clone + Send,
{
    let mut session_builder = SessionBuilder::new(config.name.clone());
    if config.ssl_acceptor.is_some() {
        session_builder.enable_start_tls();
    }
    for auth in &config.auth {
        session_builder.enable_auth(auth.clone());
    }
    let listen = if let Some(listener) = config.tcp_listener {
        listener
    } else {
        let addr = config.socket_address;
        TcpListener::bind(&addr[..])
            .map_err(|err| Error::with_source("Cannot open listen address", err))?
    };
    let server_state = ServerState {
        listener: listen,
        handler: config.handler,
        session_builder,
        ssl_acceptor: config.ssl_acceptor.map(Arc::new),
        num_threads: config.num_threads,
    };
    run(&config.name, &server_state)
}

fn run<H>(name: &str, server_state: &ServerState<H>) -> Result<(), Error>
where
    H: Handler + Clone + Send,
{
    let mut pool = Pool::new(server_state.num_threads);
    let localaddr = server_state.listener.local_addr()?;
    info!("{} SMTP started on {}", name, localaddr);
    for conn in server_state.listener.incoming() {
        let stream = conn?;
        let builder = server_state.session_builder.clone();
        let acceptor = server_state.ssl_acceptor.clone();
        let handler_clone = server_state.handler.clone();
        pool.scoped(|scope| {
            scope.execute(move || handle_connection(stream, &builder, acceptor, handler_clone));
        });
    }
    Ok(())
}

fn handle_session<H, S>(session: &mut Session<H>, stream: &mut S) -> Result<SessionResult, Error>
where
    S: BufRead + Write,
    H: Handler,
{
    let mut line = Vec::with_capacity(80);
    loop {
        line.clear();
        let num_bytes = stream.read_until(b'\n', &mut line)?;
        if num_bytes == 0 {
            break;
        }
        let res = session.process(&line);
        match res.action {
            Action::Reply => {
                write_response(stream, &res)?;
            }
            Action::Close => {
                write_response(stream, &res)?;
                if res.is_error {
                    "SMTP error".to_string();
                } else {
                    return Ok(SessionResult::Finished);
                }
            }
            Action::UpgradeTls => {
                write_response(stream, &res)?;
                return Ok(SessionResult::UpgradeTls);
            }
            Action::NoReply => (),
        }
    }
    Error::bail("Unexpected Eof")
}

fn write_response(mut writer: &mut dyn Write, res: &Response) -> Result<(), Error> {
    res.write_to(&mut writer)?;
    writer
        .flush()
        .map_err(|e| Error::with_source("Cannot write response", e))
}

fn upgrade_tls(
    stream: TcpStream,
    ssl_acceptor: Option<Arc<SslAcceptor>>,
) -> Result<SslStream<TcpStream>, Error> {
    if let Some(acceptor) = ssl_acceptor {
        let ret = acceptor
            .accept(stream)
            .map_err(|e| Error::with_source("Cannot upgrade to TLS", e))?;
        trace!("Upgrade TLS successful");
        Ok(ret)
    } else {
        Error::bail("Cannot upgrade to TLS without an SslAcceptor")
    }
}

fn start_session<H: Handler>(
    session_builder: &SessionBuilder,
    remote: IpAddr,
    mut stream: BufStream<TcpStream>,
    ssl_acceptor: Option<Arc<SslAcceptor>>,
    handler: H,
) -> Result<(), Error> {
    // TODO: have an embedded relay server with authentication
    let mut session = session_builder.build(remote, handler);
    write_response(&mut stream, &session.greeting())?;
    let res = handle_session(&mut session, &mut stream)?;
    if let SessionResult::UpgradeTls = res {
        let inner_stream = stream
            .into_inner()
            .map_err(|e| Error::with_source("Cannot flush original TcpStream", e))?;
        let tls = upgrade_tls(inner_stream, ssl_acceptor)?;
        session.tls_active();
        let mut buf_tls = BufStream::new(tls);
        handle_session(&mut session, &mut buf_tls)?;
    }
    Ok(())
}

fn handle_connection<H: Handler>(
    stream: TcpStream,
    session_builder: &SessionBuilder,
    ssl_acceptor: Option<Arc<SslAcceptor>>,
    handler: H,
) {
    let remote = stream
        .peer_addr()
        .map(|saddr| saddr.ip())
        .unwrap_or_else(|_| "0.0.0.0".parse().unwrap());
    debug!("New connection from {}", remote);
    stream.set_read_timeout(Some(*FIVE_MINUTES)).ok();
    stream.set_write_timeout(Some(*FIVE_MINUTES)).ok();
    let bufstream = BufStream::new(stream);
    if let Err(err) = start_session(&session_builder, remote, bufstream, ssl_acceptor, handler) {
        error!("({}) {}", remote, err);
    }
}
