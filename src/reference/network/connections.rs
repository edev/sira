//! Implementation details of [Network].

use super::{ClientThread, Report};
use crate::executor::NetworkControlMessage;
#[cfg(doc)]
use crate::reference::network::Network;
use crossbeam::channel::{self, Receiver, Sender};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::thread::JoinHandle;

/// Per-client data that [Network] needs to store to manage clients and their threads.
struct Client {
    /// The join handle for this client's thread.
    thread: JoinHandle<()>,

    /// The [Sender] that the network controller ([Network]) uses to communicate with this specific
    /// client.
    outbox: Sender<NetworkControlMessage>,
}

/// A collection that facilitates managing threads and message-passing for client connections.
pub struct Connections<CT: ClientThread> {
    /// The [Sender] that will be cloned into each client connection thread to send reports to
    /// [Connections::inbox].
    client_outbox: Sender<Report>,

    /// Maps host names to [Client] values for all running client connection threads.
    connections: HashMap<String, Client>,

    /// The shared [Receiver] for [Report]s from all client connections.
    pub inbox: Receiver<Report>,

    #[doc(hidden)]
    // We call CT::run from sender_for but don't store any CT values.
    client_thread: PhantomData<CT>,
}

impl<CT: ClientThread> Connections<CT> {
    /// Creates an empty [Connections] table.
    pub fn new() -> Self {
        let (client_outbox, inbox) = channel::unbounded();
        Connections {
            connections: HashMap::new(),
            inbox,
            client_outbox,
            client_thread: PhantomData,
        }
    }

    /// Sends a [NetworkControlMessage] to a client thread.
    ///
    /// If no thread exists for the specified host, spawns a new one and delivers the message.
    fn send(&mut self, message: NetworkControlMessage) {
        use NetworkControlMessage::*;
        let host = match &message {
            RunAction(host_action) => self.sender_for(host_action.host()),
            Disconnect(host) => self.sender_for(host),
        };
        todo!();
    }

    /// Returns the [Sender] for sending a [NetworkControlMessage] to the thread for `host`.
    ///
    /// If no such thread exist, creates the thread and the corresponding mapping in
    /// [Self::connections].
    fn sender_for<H>(&mut self, host: &H) -> &Sender<NetworkControlMessage>
    where
        String: Borrow<H>,
        H: ?Sized,
    {
        todo!()
    }
}
