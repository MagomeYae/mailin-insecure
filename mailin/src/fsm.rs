use crate::parser::{decode_sasl_login, decode_sasl_plain, parse, parse_auth_response};
use crate::response::*;

use crate::smtp::Cmd;
use crate::{AuthMechanism, Handler, Response};
use either::*;
use log::{error, trace};
use std::borrow::BorrowMut;
use std::net::IpAddr;
use ternop::ternary;

#[cfg(test)]
#[derive(Debug)]
pub(crate) enum SmtpState {
    Invalid,
    Idle,
    Hello,
    HelloAuth,
    Auth,
    Mail,
    Rcpt,
    Data,
}

#[derive(PartialEq)]
enum TlsState {
    Unavailable,
    Inactive,
    Active,
}

enum AuthState {
    Unavailable,
    RequiresAuth,
    Authenticated,
}

trait State<H: Handler>: Send + Sync {
    #[cfg(test)]
    fn id(&self) -> SmtpState;

    // Handle an incoming command and return the next state
    fn handle(
        self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>);

    // Most state will convert an input line into a command.
    // Some states, e.g Data, need to process input lines differently and will
    // override this method.
    fn process_line<'a>(&mut self, _handler: &mut H, line: &'a [u8]) -> Either<Cmd<'a>, Response> {
        trace!("> {}", String::from_utf8_lossy(line));
        parse(line).map(Left).unwrap_or_else(Right)
    }
}

//------------------------------------------------------------------------------

// Return the next state depending on the response
fn next_state<F, H>(
    current: Box<dyn State<H>>,
    res: Response,
    next_state: F,
) -> (Response, Option<Box<dyn State<H>>>)
where
    F: FnOnce() -> Box<dyn State<H>>,
{
    if res.action == Action::Close {
        (res, None)
    } else if res.is_error {
        (res, Some(current))
    } else {
        (res, Some(next_state()))
    }
}

// Convert the current state to the next state depending on the response
fn transform_state<S, F, H: Handler>(
    current: Box<S>,
    res: Response,
    next_state: F,
) -> (Response, Option<Box<dyn State<H>>>)
where
    S: State<H> + 'static,
    F: FnOnce(S) -> Box<dyn State<H>>,
{
    if res.action == Action::Close {
        (res, None)
    } else if res.is_error {
        (res, Some(current))
    } else {
        (res, Some(next_state(*current)))
    }
}

fn default_handler<H: Handler>(
    current: Box<dyn State<H>>,
    fsm: &StateMachine<H>,
    handler: &mut H,
    cmd: &Cmd,
) -> (Response, Option<Box<dyn State<H>>>) {
    match *cmd {
        Cmd::Quit => (GOODBYE, None),
        Cmd::Helo { domain } => handle_helo(current, fsm, handler, domain),
        Cmd::Ehlo { domain } => handle_ehlo(current, fsm, handler, domain),
        Cmd::Noop => (OK, Some(current)),
        _ => unhandled(current),
    }
}

fn unhandled<H>(current: Box<dyn State<H>>) -> (Response, Option<Box<dyn State<H>>>) {
    (BAD_SEQUENCE_COMMANDS, Some(current))
}

fn handle_rset<H: Handler>(
    fsm: &StateMachine<H>,
    domain: &str,
) -> (Response, Option<Box<dyn State<H>>>) {
    match fsm.auth_state {
        AuthState::Unavailable => (
            OK,
            Some(Box::new(Hello {
                domain: domain.to_string(),
            })),
        ),
        _ => (
            OK,
            Some(Box::new(HelloAuth {
                domain: domain.to_string(),
            })),
        ),
    }
}

fn handle_helo<H: Handler>(
    current: Box<dyn State<H>>,
    fsm: &StateMachine<H>,
    handler: &mut H,
    domain: &str,
) -> (Response, Option<Box<dyn State<H>>>) {
    match fsm.auth_state {
        AuthState::Unavailable => {
            let res = handler.helo(fsm.ip, domain);
            next_state(current, res, || {
                Box::new(Hello {
                    domain: domain.to_owned(),
                })
            })
        }
        _ => {
            // If authentication is required the client should be using EHLO
            (BAD_HELLO, Some(current))
        }
    }
}

fn handle_ehlo<H: Handler>(
    current: Box<dyn State<H>>,
    fsm: &StateMachine<H>,
    handler: &mut H,
    domain: &str,
) -> (Response, Option<Box<dyn State<H>>>) {
    let mut res = handler.helo(fsm.ip, domain);
    if res.code == 250 {
        res = fsm.ehlo_response();
    }
    match fsm.auth_state {
        AuthState::Unavailable => next_state(current, res, || {
            Box::new(Hello {
                domain: domain.to_owned(),
            })
        }),
        AuthState::RequiresAuth | AuthState::Authenticated => next_state(current, res, || {
            Box::new(HelloAuth {
                domain: domain.to_owned(),
            })
        }),
    }
}

fn authenticate_plain<H: Handler>(
    fsm: &mut StateMachine<H>,
    handler: &mut H,
    authorization_id: &str,
    authentication_id: &str,
    password: &str,
) -> Response {
    let auth_res = handler.auth_plain(authorization_id, authentication_id, password);
    fsm.auth_state = ternary!(
        auth_res.code == 235,
        AuthState::Authenticated,
        AuthState::RequiresAuth
    );
    auth_res
}

fn authenticate_login<H: Handler>(
    fsm: &mut StateMachine<H>,
    handler: &mut H,
    username: &str,
    password: &str,
) -> Response {
    let auth_res = handler.auth_login(username, password);
    fsm.auth_state = ternary!(
        auth_res.code == 235,
        AuthState::Authenticated,
        AuthState::RequiresAuth
    );
    auth_res
}

//------------------------------------------------------------------------------

struct Idle {}

impl<H: Handler> State<H> for Idle {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::Idle
    }

    fn handle(
        self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::StartedTls => {
                fsm.tls = TlsState::Active;
                (EMPTY_RESPONSE, Some(self))
            }
            Cmd::Rset => (OK, Some(self)),
            _ => default_handler(self, fsm, handler, &cmd),
        }
    }
}

//------------------------------------------------------------------------------

struct Hello {
    domain: String,
}

impl<H: Handler> State<H> for Hello {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::Hello
    }

    fn handle(
        self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::Mail {
                reverse_path,
                is8bit,
            } => {
                let res = handler.mail(fsm.ip, &self.domain, reverse_path);
                transform_state(self, res, |s| {
                    Box::new(Mail {
                        domain: s.domain,
                        reverse_path: reverse_path.to_owned(),
                        is8bit,
                    })
                })
            }
            Cmd::StartTls if fsm.tls == TlsState::Inactive => (START_TLS, Some(Box::new(Idle {}))),
            Cmd::Vrfy => (VERIFY_RESPONSE, Some(self)),
            Cmd::Rset => handle_rset(fsm, &self.domain),
            _ => default_handler(self, fsm, handler, &cmd),
        }
    }
}

//------------------------------------------------------------------------------

struct HelloAuth {
    domain: String,
}

impl<H: Handler> State<H> for HelloAuth {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::HelloAuth
    }

    fn handle(
        self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::StartTls => (START_TLS, Some(Box::new(Idle {}))),
            Cmd::AuthPlain {
                ref authorization_id,
                ref authentication_id,
                ref password,
            } if fsm.allow_auth_plain() => {
                let res =
                    authenticate_plain(fsm, handler, authorization_id, authentication_id, password);
                transform_state(self, res, |s| Box::new(Hello { domain: s.domain }))
            }
            Cmd::AuthPlainEmpty if fsm.allow_auth_plain() => {
                let domain = self.domain.clone();
                (
                    EMPTY_AUTH_CHALLENGE,
                    Some(Box::new(Auth {
                        domain,
                        mechanism: AuthMechanism::Plain,
                        username: None,
                    })),
                )
            }
            Cmd::AuthLogin { ref username } if fsm.allow_auth_login() => {
                let domain = self.domain.clone();
                (
                    PASSWORD_AUTH_CHALLENGE,
                    Some(Box::new(Auth {
                        domain,
                        mechanism: AuthMechanism::Login,
                        username: Some(username.clone()),
                    })),
                )
            }
            Cmd::AuthLoginEmpty if fsm.allow_auth_login() => {
                let domain = self.domain.clone();
                (
                    USERNAME_AUTH_CHALLENGE,
                    Some(Box::new(Auth {
                        domain,
                        mechanism: AuthMechanism::Login,
                        username: None,
                    })),
                )
            }
            Cmd::Rset => handle_rset(fsm, &self.domain),
            _ => default_handler(self, fsm, handler, &cmd),
        }
    }
}

//------------------------------------------------------------------------------

struct Auth {
    domain: String,
    mechanism: AuthMechanism,
    username: Option<String>,
}

impl<H: Handler> State<H> for Auth {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::Auth
    }

    fn handle(
        mut self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::AuthResponse { response } => match self.mechanism {
                AuthMechanism::Plain => {
                    let creds = decode_sasl_plain(response);
                    let res = authenticate_plain(
                        fsm,
                        handler,
                        &creds.authorization_id,
                        &creds.authentication_id,
                        &creds.password,
                    );
                    if res.is_error {
                        (
                            res,
                            Some(Box::new(HelloAuth {
                                domain: self.domain,
                            })),
                        )
                    } else {
                        (
                            res,
                            Some(Box::new(Hello {
                                domain: self.domain,
                            })),
                        )
                    }
                }
                AuthMechanism::Login => {
                    let credential = decode_sasl_login(response);
                    if let Some(username) = self.username {
                        let res = authenticate_login(fsm, handler, &username, &credential);
                        let domain = self.domain.clone();
                        if res.is_error {
                            (res, Some(Box::new(HelloAuth { domain })))
                        } else {
                            (res, Some(Box::new(Hello { domain })))
                        }
                    } else {
                        self.username = Some(credential);
                        (PASSWORD_AUTH_CHALLENGE, Some(self))
                    }
                }
            },
            _ => unhandled(self),
        }
    }

    fn process_line<'a>(&mut self, _handler: &mut H, line: &'a [u8]) -> Either<Cmd<'a>, Response> {
        trace!("> {}", String::from_utf8_lossy(line));
        parse_auth_response(line)
            .map(|r| Left(Cmd::AuthResponse { response: r }))
            .unwrap_or_else(Right)
    }
}

//------------------------------------------------------------------------------

struct Mail {
    domain: String,
    reverse_path: String,
    is8bit: bool,
}

impl<H: Handler> State<H> for Mail {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::Mail
    }

    fn handle(
        self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::Rcpt { forward_path } => {
                let res = handler.rcpt(forward_path);
                transform_state(self, res, |s| {
                    let fp = vec![forward_path.to_owned()];
                    Box::new(Rcpt {
                        domain: s.domain,
                        reverse_path: s.reverse_path,
                        is8bit: s.is8bit,
                        forward_path: fp,
                    })
                })
            }
            Cmd::Rset => handle_rset(fsm, &self.domain),
            _ => default_handler(self, fsm, handler, &cmd),
        }
    }
}

//------------------------------------------------------------------------------

struct Rcpt {
    domain: String,
    reverse_path: String,
    is8bit: bool,
    forward_path: Vec<String>,
}

impl<H: Handler> State<H> for Rcpt {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::Rcpt
    }

    fn handle(
        self: Box<Self>,
        fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::Data => {
                let res = handler.data_start(
                    &self.domain,
                    &self.reverse_path,
                    self.is8bit,
                    &self.forward_path,
                );
                let res = ternary!(res.is_error, res, START_DATA);
                transform_state(self, res, |s| {
                    Box::new(Data {
                        domain: s.domain,
                        has_error: false,
                    })
                })
            }
            Cmd::Rcpt { forward_path } => {
                let res = handler.rcpt(forward_path);
                transform_state(self, res, |s| {
                    let mut fp = s.forward_path;
                    fp.push(forward_path.to_owned());
                    Box::new(Rcpt {
                        domain: s.domain,
                        reverse_path: s.reverse_path,
                        is8bit: s.is8bit,
                        forward_path: fp,
                    })
                })
            }
            Cmd::Rset => handle_rset(fsm, &self.domain),
            _ => default_handler(self, fsm, handler, &cmd),
        }
    }
}

//------------------------------------------------------------------------------

struct Data {
    domain: String,
    has_error: bool,
}

impl<H: Handler> State<H> for Data {
    #[cfg(test)]
    fn id(&self) -> SmtpState {
        SmtpState::Data
    }

    fn handle(
        self: Box<Self>,
        _fsm: &mut StateMachine<H>,
        handler: &mut H,
        cmd: Cmd,
    ) -> (Response, Option<Box<dyn State<H>>>) {
        match cmd {
            Cmd::DataEnd => {
                let res = if self.has_error {
                    // the error was already reported, do not send it twice
                    EMPTY_RESPONSE
                } else {
                    handler.data_end()
                };
                transform_state(self, res, |s| Box::new(Hello { domain: s.domain }))
            }
            _ => unhandled(self),
        }
    }

    fn process_line<'a>(
        &mut self,
        handler: &mut H,
        mut line: &'a [u8],
    ) -> Either<Cmd<'a>, Response> {
        if line == b".\r\n" {
            trace!("> _data_");
            Left(Cmd::DataEnd)
        } else if self.has_error {
            // there was an error, stop processing
            Right(EMPTY_RESPONSE)
        } else {
            if line.starts_with(b".") {
                line = &line[1..];
            }
            match handler.data(line) {
                Ok(_) => Right(EMPTY_RESPONSE),
                Err(e) => {
                    error!("Error saving message: {}", e);
                    self.has_error = true;
                    Right(TRANSACTION_FAILED)
                }
            }
        }
    }
}
//------------------------------------------------------------------------------

pub(crate) struct StateMachine<H: Handler> {
    ip: IpAddr,
    auth_mechanisms: Vec<AuthMechanism>,
    auth_state: AuthState,
    tls: TlsState,
    smtp: Option<Box<dyn State<H>>>,
    auth_plain: bool,
    auth_login: bool,
    insecure_allow_plaintext_auth: bool,
}

impl<H: Handler> StateMachine<H> {
    pub fn new(
        ip: IpAddr,
        auth_mechanisms: Vec<AuthMechanism>,
        allow_start_tls: bool,
        insecure_allow_plaintext_auth: bool,
    ) -> Self {
        let auth_state = ternary!(
            auth_mechanisms.is_empty(),
            AuthState::Unavailable,
            AuthState::RequiresAuth
        );
        let tls = ternary!(allow_start_tls, TlsState::Inactive, TlsState::Unavailable);
        let auth_plain = auth_mechanisms.contains(&AuthMechanism::Plain);
        let auth_login = auth_mechanisms.contains(&AuthMechanism::Login);
        Self {
            ip,
            auth_mechanisms,
            auth_state,
            tls,
            smtp: Some(Box::new(Idle {})),
            auth_plain,
            auth_login,
            insecure_allow_plaintext_auth,
        }
    }

    // Respond and change state with the given command
    pub fn command(&mut self, handler: &mut H, cmd: Cmd) -> Response {
        let (response, next_state) = match self.smtp.take() {
            Some(last_state) => last_state.handle(self, handler, cmd),
            None => (INVALID_STATE, None),
        };
        self.smtp = next_state;
        response
    }

    pub fn process_line<'a>(
        &mut self,
        handler: &mut H,
        line: &'a [u8],
    ) -> Either<Cmd<'a>, Response> {
        match self.smtp {
            Some(ref mut s) => {
                let s: &mut dyn State<H> = s.borrow_mut();
                s.process_line(handler, line)
            }
            None => Right(INVALID_STATE),
        }
    }

    #[cfg(test)]
    pub fn current_state(&self) -> SmtpState {
        let id = self.smtp.as_ref().map(|s| s.id());
        id.unwrap_or(SmtpState::Invalid)
    }

    fn ehlo_response(&self) -> Response {
        let mut extensions = vec!["8BITMIME".to_string()];
        if self.tls == TlsState::Inactive {
            extensions.push("STARTTLS".to_string());
        }

        if self.allow_auth() && !self.auth_mechanisms.is_empty() {
            let mut auth_available = "AUTH".to_string();
            for auth in &self.auth_mechanisms {
                auth_available += " ";
                auth_available += auth.extension();
            }
            extensions.push(auth_available);
        }
        Response::dynamic(250, "server offers extensions:".to_string(), extensions)
    }

    fn allow_auth_plain(&self) -> bool {
        self.auth_plain && self.allow_auth()
    }

    fn allow_auth_login(&self) -> bool {
        self.auth_login && self.allow_auth()
    }

    fn allow_auth(&self) -> bool {
        self.insecure_allow_plaintext_auth || (self.tls == TlsState::Active)
    }
}
