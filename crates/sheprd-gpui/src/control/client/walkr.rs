//! The Walkr commands' wire calls and text output.
use super::{Context, ErrorCode, Output, unexpected};
use crate::{
    cli::control::{RunSource, WalkrCommand},
    control::protocol::{
        Request, Response,
        walkr::{Connectivity, HostSessions, Reply, RunAddress, Source, Unreachable, WalkrOp},
    },
};
use std::io::BufRead;

pub(super) fn run(command: WalkrCommand, context: &Context) -> crate::Result<Output> {
    match command {
        WalkrCommand::Status => status(context),
        WalkrCommand::Connect { address, show } => {
            let address = match address {
                RunSource::Argument(address) => address,
                RunSource::Stdin => first_line(std::io::stdin().lock())?,
            };
            connect(address, show, context)
        }
        WalkrCommand::Disconnect => context.act(Request::Walkr, WalkrOp::Disconnect {}),
    }
}

/// The first line of `input`, without its line end. The read ends at the
/// first line end: a writer that keeps its pipe open is not waited for. One
/// byte more than the app accepts is read at most, so that the app refuses
/// an over-long address itself and a stream without end stops here.
fn first_line(input: impl BufRead) -> crate::Result<RunAddress> {
    let mut bytes = Vec::new();
    input
        .take(RunAddress::MAX_BYTES as u64 + 2)
        .read_until(b'\n', &mut bytes)?;
    let line = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    // The error names no byte of the input: it may be most of an address.
    String::from_utf8(line.to_vec())
        .map(RunAddress::new)
        .map_err(|_| {
            crate::control::Error::Rejected {
                code: ErrorCode::InvalidUrl,
                message: "The Walkr address on standard input must be UTF-8".into(),
            }
            .into()
        })
}

fn connect(address: RunAddress, show: bool, context: &Context) -> crate::Result<Output> {
    let op = WalkrOp::Connect { address, show };
    match context.call(Request::Walkr, op)? {
        Response::Walkr {
            reply: reply @ Reply::Connected { .. },
        } => {
            let Reply::Connected { address, replaced } = &reply else {
                return Err(crate::control::Error::NoResponse.into());
            };
            let replaced = if *replaced {
                "\nIt replaced the run of another Walkr server; that page is closed."
            } else {
                ""
            };
            Output::data(format!("Connected Walkr at {address}{replaced}"), &reply)
        }
        response => Err(unexpected(response)),
    }
}

fn status(context: &Context) -> crate::Result<Output> {
    let status = match context.call(Request::Walkr, WalkrOp::Status {})? {
        Response::Walkr {
            reply: Reply::Status(status),
        } => status,
        response => return Err(unexpected(response)),
    };
    let saved = sessions_text(&status.sessions);
    if status.source == Source::None {
        return Ok(Output::data(
            format!(
                "No Walkr run is connected and walkr.url is unset. An agent connects one with sheprd walkr connect.{saved}"
            ),
            &status,
        )?
        .nothing());
    }
    let address = &status.address;
    let state = match status.connectivity {
        Connectivity::Down { status } => format!("{address} answers with {status}"),
        Connectivity::Unexpected { status } => format!(
            "Unexpected Walkr response ({status}); check walkr.url and walkr.probe_path: {address}"
        ),
        state => format!("{}: {address}", state_text(state)),
    };
    let age = status.checked_seconds_ago.map_or_else(
        || "No probe has answered yet".into(),
        |seconds| format!("Last checked {seconds} seconds ago"),
    );
    let source = match (status.source, status.live) {
        (Source::Run, true) => "\nA connected run, live",
        (Source::Run, false) => "\nA connected run that ended; its page stays open",
        (Source::Override, _) => "\nThe walkr.url setting",
        (Source::None, _) => "",
    };
    Output::data(format!("{state}\n{age}{source}{saved}"), &status)
}

/// One line for each host with its counts of saved sessions and of those
/// that wait for a pick. Counts only: the app sends no title or text of a
/// session.
fn sessions_text(sessions: &[HostSessions]) -> String {
    sessions
        .iter()
        .map(|host| {
            let (count, waiting) = (host.count, host.waiting);
            match host.host.as_str() {
                "local" => format!(
                    "\nSaved Compare sessions on this computer: {count}, waiting for a pick: {waiting}"
                ),
                name => format!(
                    "\nSaved Compare sessions on {name}: {count}, waiting for a pick: {waiting}"
                ),
            }
        })
        .collect()
}

/// The state in words, for a state the address follows.
fn state_text(state: Connectivity) -> &'static str {
    match state {
        Connectivity::NotConfigured => "Walkr is not configured",
        Connectivity::Checking => "Checking Walkr",
        Connectivity::Unreachable { cause } => match cause {
            Unreachable::Dns => "Cannot reach Walkr: name not found; check walkr.url",
            Unreachable::Connect => "Cannot reach Walkr: no connection",
            Unreachable::Timeout => "Cannot reach Walkr: no answer in 5 seconds",
            Unreachable::Tls => "Cannot reach Walkr: the TLS certificate or handshake was refused",
        },
        Connectivity::NetworkRefused => "Walkr refuses this address (its Host rule)",
        Connectivity::Down { .. } => "The Walkr server answers with an error",
        Connectivity::Unexpected { .. } => "Unexpected Walkr response",
        Connectivity::Reachable => "Walkr answers",
        Connectivity::Ready => "Walkr page shown",
    }
}
