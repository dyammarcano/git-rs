mod dispatch;
mod git_command;

use self::dispatch::dispatch;
use futures::future;
use futures::future::{loop_fn, Future, Loop};
use message;
use semver::Version;
use state;
use std::sync::{Arc, Mutex};
use tokio;
use tokio::net::TcpStream;
use util::transport::{read_message, send_message, Transport};

macro_rules! read_validated_message {
    ($messagePattern:pat, $connection_state:expr) => {
        read_message($connection_state).and_then(|(response, connection_state)| {
            use error::protocol::{Error, InboundMessageError};

            match response {
                $messagePattern => Ok((response, connection_state)),
                _ => Err(Error::InboundMessage(InboundMessageError::Unexpected)),
            }
        })
    };
}

pub fn init_dispatch(state: Arc<Mutex<state::Shared>>, socket: TcpStream) {
    use message::protocol::{Inbound, Outbound};
    let transport = Transport::new(socket);
    let connection_state = state::Connection::new(state, transport);

    let connection = send_message(
        connection_state,
        Outbound::Hello {
            version: Version::new(0, 1, 0),
        },
    ).and_then(|connection_state| {
        println!("wrote hello message");
        read_validated_message!(Inbound::Hello, connection_state)
    })
        .and_then(|(_, connection_state)| send_message(connection_state, Outbound::GladToMeetYou))
        .and_then(|connection_state| {
            loop_fn(connection_state, |connection_state| {
                read_message(connection_state).and_then(
                    |(response, connection_state)| -> Box<
                        Future<
                                Item = Loop<state::Connection, state::Connection>,
                                Error = ::error::protocol::Error,
                            >
                            + Send,
                    > {
                        if let Inbound::Goodbye = response {
                            Box::new(future::ok(Loop::Break(connection_state)))
                        } else {
                            Box::new(dispatch(connection_state, response).map(Loop::Continue))
                        }
                    },
                )
            })
        })
        .and_then(|transport| send_message(transport, Outbound::Goodbye { error_code: None }))
        .and_then(|_| Ok(()))
        .map_err(|err| println!("error; err={:?}", err));

    tokio::spawn(connection);
}