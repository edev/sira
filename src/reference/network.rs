//! Reference [crate::network] implementations.

#[cfg(doc)]
use crate::core::action::Action;
#[cfg(doc)]
use crate::executor::Executor;
use crate::executor::NetworkControlMessage;
use crate::logger::Log;
use crate::network::{ChannelPair, Report};
use crossbeam::channel::{Receiver, Sender, TryRecvError};

mod connections;
use connections::Connections;

#[cfg(feature = "openssh")]
pub mod openssh;

/// Implement this to define your own network provider.
///
/// [ClientThread] values are meant to be moved into client threads and hold everything a client
/// thread needs to know or do.
///
/// See [crate::reference::network::openssh] for an example.
pub trait ClientThread {
    /// Creates a new [ClientThread] value and returns it.
    ///
    /// Does not spawn a new thread or perform any other work.
    fn new(host: String, sender: Sender<Report>, receiver: Receiver<NetworkControlMessage>)
        -> Self;

    /// Runs a client's logic, blocking until it's done.
    ///
    /// [Network] will spawn a thread for each client and call this method from that thread. The
    /// code in this method needs to connect to the remote host and act on any
    /// [NetworkControlMessage]s. For [NetworkControlMessage::RunAction] messages, this means
    /// either invoking `sira-client` on the remote host or handling the actions directly. For
    /// instance, `sira-client` is not responsible for [Action::Upload] and [Action::Download],
    /// because these are better suited to a controller-side utility such as `scp`.
    fn run(self);
}

/// A generic network implementation that works for any [ClientThread].
///
/// # One thread per connection
///
/// Each [ClientThread::run] invocation runs in its own thread, so this model uses one thread per
/// connection. This minimizes external dependencies, produces simpler and more obviously correct
/// code, and scales fine for small deployments. If you wish to use multi-threading in your
/// [ClientThread] implementation, you are free to do so.
///
/// On the other hand, if opening one thread per connection is unacceptable for your use case, you
/// will need to implement your own network stack starting from [crate::network]. In that case,
/// though, Sira might not be a good fit for your project, as you might run into scaling issues
/// elsewhere as well.
pub struct Network<CT: ClientThread> {
    /// Channels for sending reports to [Executor] and receiving [NetworkControlMessage]s from
    /// [Executor].
    executor: ChannelPair,

    /// A connection to the logger for logging information not covered by [Self::executor].
    #[allow(unused)]
    log: Log,

    /// Handles and stores client threads, connection states, and so on.
    connections: Connections<CT>,
}

impl<CT: ClientThread> Network<CT> {
    /// Creates a ready-to-run [Network]. Run it with [Network::run()].
    pub fn new(executor: ChannelPair, log: Log) -> Self {
        let connections = Connections::new();
        Network {
            executor,
            log,
            connections,
        }
    }

    /// Starts the network, blocking until the program is getting ready to quit.
    ///
    /// You'll probably want to start this in its own thread.
    pub fn run(self) -> anyhow::Result<()> {
        // Crossbeam's recv only returns an Err if the channel is empty and disconnected. If the
        // executor Receiver returns an error, this is not an error state, from our perspective:
        // it simply means it's time to quit.
        //
        // Crossbeam's send works similarly. However, if we fail to send a message to a client
        // thread, this is an error state and indicates a bug. It means that a client dropped its
        // Receiver without being instructed to do so, perhaps due to a crash or a logic error.
        // Therefore, we must panic.
        //
        // In the event that a client disconnects (i.e. sends a Report indicating a disconnection),
        // either because Executor requested it or because of an issue with the connection, the
        // code here should drop the Client value from Connections, allowing that thread to close.
        // It should then join the thread's handle to allow the thread's memory to be cleared.
        // If warranted, we can then open a new thread and a new connection to retry, but that is
        // not currently impelemented.

        loop {
            // Receive any available client Reports, but don't block. We want to prioritize
            // client Reports so that we have the most up-to-date information available before
            // acting on any incoming NetworkControlMessages.
            //
            // This reduces the potential for race conditions on messages between the two channels,
            // but it's still possible for race conditions to arise, either among messages on the
            // two channels or actual states of the different parts of the program, since the
            // system is in constant, network-connected flux.
            match self.connections.inbox.try_recv() {
                Ok(report) => {
                    todo!();

                    // Skip checking the executor Receiver in case there are more inbox messages.
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => todo!(),
            }

            // Receive any available instructions from executor, but, like above, don't block, as
            // blocking would disrupt the priority describe above.
            match self.executor.receiver.try_recv() {
                _ => todo!(),
            }

            // If neither Receiver is ready, then block until a channel is ready. Then, simply resume
            // the loop, checking messages using the logic above.
            todo!()
        }

        // TODO Block until network connections are done with their current actions? E.g. send
        // NetworkControlMessage::Disconnect to all hosts and then join the threads?
        Ok(())
    }
}
