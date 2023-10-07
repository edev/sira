//! Reference [crate::network] implementations.

#[cfg(doc)]
use crate::core::action::Action;
#[cfg(doc)]
use crate::executor::Executor;
use crate::executor::NetworkControlMessage;
use crate::logger::Log;
use crate::network::{ChannelPair, Report};
use crossbeam::channel::{self, Receiver, Select, Sender, TryRecvError};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::thread::{self, JoinHandle};

#[cfg(feature = "openssh")]
pub mod openssh;

/// Per-client data that [Network] needs to store to manage clients and their threads.
struct Client {
    /// The join handle for this client's thread.
    thread: JoinHandle<()>,

    /// The [Sender] that the network controller ([Network]) uses to communicate with this specific
    /// client.
    outbox: Sender<NetworkControlMessage>,
}

/// Creates and runs a thread to manage a single remote host.
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
    /// The [Sender] that will be cloned into each client connection thread to send reports to
    /// [Self::inbox].
    client_outbox: Sender<Report>,

    #[doc(hidden)]
    // We call CT::run from sender_for but don't store any CT values.
    client_thread: PhantomData<CT>,

    /// Maps host names to [Client] values for all running client connection threads.
    connections: HashMap<String, Client>,
    /// Channels for sending reports to [Executor] and receiving [NetworkControlMessage]s from
    /// [Executor].
    executor: ChannelPair,

    /// The shared [Receiver] for [Report]s from all client connections.
    inbox: Receiver<Report>,

    /// A connection to the logger for logging information not covered by [Self::executor].
    #[allow(unused)]
    log: Log,
}

impl<CT: ClientThread> Network<CT> {
    /// Creates a ready-to-run [Network]. Run it with [Network::run()].
    pub fn new(executor: ChannelPair, log: Log) -> Self {
        let (client_outbox, inbox) = channel::unbounded();
        Network {
            client_outbox,
            client_thread: PhantomData,
            connections: HashMap::new(),
            executor,
            inbox,
            log,
        }
    }

    /// Starts the network, blocking until the program is getting ready to quit.
    ///
    /// You'll probably want to start this in its own thread.
    pub fn run(mut self) -> anyhow::Result<()> {
        loop {
            // Crossbeam's [Select::ready] warns that it "might return with success spuriously". If
            // this happens, we will simply find ourselves back here after passing harmlessly
            // through the logic below. We could use more complex logic here to ensure that a
            // channel really is ready, but there's no added value in doing so.
            //
            // Note that this behavior is appears infeasible to unit test. Moving it out of
            // `_run_once` and into `run` guarantees termination for `_run_once`, which is what the
            // unit tests run.
            let mut select = Select::new();
            select.recv(&self.inbox);
            select.recv(&self.executor.receiver);
            select.ready();

            let (should_continue, retval) = self._run_once(&mut NetworkRun());
            if !should_continue {
                // TODO Block until network connections are done with their current actions? E.g.
                // send NetworkControlMessage::Disconnect to all hosts and then join the threads?
                return retval;
            }
        }
    }

    /// A single iteration of the [Self::run] loop, broken out for better testing.
    ///
    /// Specifically, this method allows for step-by-step evaluation and guarantees termination.
    /// Additionally, it allows us to inject a [Run] for unit test isolation.
    ///
    /// # Returns
    ///
    /// Returns whether to continue looping and what the caller should return if it terminates
    /// rather than looping.
    fn _run_once<R: Run<CT>>(&mut self, run: &mut R) -> (bool, anyhow::Result<()>) {
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
        // code here should drop the Client value from connections, allowing that thread to close.
        // It should then join the thread's handle to allow the thread's memory to be cleared.
        // If warranted, we can then open a new thread and a new connection to retry, but that is
        // not currently implemented.

        // Receive any available client Reports, but don't block. We want to prioritize
        // client Reports so that we have the most up-to-date information available before
        // acting on any incoming NetworkControlMessages.
        //
        // This reduces the potential for race conditions on messages between the two channels,
        // but it's still possible for race conditions to arise, either among messages on the
        // two channels or actual states of the different parts of the program, since the
        // system is in constant, network-connected flux.
        match self.inbox.try_recv() {
            Ok(report) => {
                if let Report::FailedToConnect { host, error } = &report {
                    run.disconnect_client(self, host, &Some(error));
                } else if let Report::Disconnected { host, error } = &report {
                    run.disconnect_client(self, host, error);
                }

                match self.executor.sender.send(report) {
                    Ok(_) => {
                        // Skip checking executor.receiver in case there are more inbox messages.
                        return (true, Ok(()));
                    }
                    Err(_) => {
                        // Executor's gone, which means it's time to quit.
                        return (false, Ok(()));
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                // This should absolutely never happen, because `self.client_outbox` holds a Sender
                // that goes to `self.inbox`. If it happens, definitely panic!
                panic!(
                    "All network client Senders disconnected. This should be impossible. \
                    Please report this bug!"
                );
            }
        }

        // Receive any available instructions from executor, but, like above, don't block, as
        // blocking would disrupt the priority describe above.
        match self.executor.receiver.try_recv() {
            Ok(message) => run.send(self, message),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return (false, Ok(())),
        }

        (true, Ok(()))
    }

    /// Returns the [Sender] for sending a [NetworkControlMessage] to the thread for `host`.
    ///
    /// If no such thread exists, creates the thread and the corresponding mapping in
    /// [Self::connections].
    ///
    /// Used by [NetworkRun::send].
    fn client_for<H>(&mut self, host: &H) -> &Client
    where
        String: Borrow<H>,
        H: ToString + Hash + Eq + ?Sized,
    {
        // Using the Entry API here would require calling `host.to_string()` (or accepting an owned
        // String, which would have the same effect on performance). Almost all invocations of this
        // method will be for hosts that are in the collection, and the collection is quite small.
        // Therefore, it should be much faster to simply take a reference and insert if missing.

        // Note: writing this method with the opposite control flow, i.e. first checking for an
        // existing client and returning early if it exists, appears to confuse the borrow checker
        // (at time of writing).

        if !self.connections.contains_key(host) {
            // Spawn a new client thread.
            let client_host = host.to_string();
            let client_sender = self.client_outbox.clone();
            let (outbox, client_receiver) = channel::unbounded();
            let thread =
                thread::spawn(|| CT::new(client_host, client_sender, client_receiver).run());

            self.connections
                .insert(host.to_string(), Client { thread, outbox });
        }
        self.connections.get(host).unwrap()
    }
}

/// Breaks out the methods that [Network::run] calls for easier isolation and testing.
///
/// # Design notes
///
/// We can't impelement this on [Network], because that would require us to pass mutable references
/// to `self` twice, e.g. `self.send(self, ...)`, which isn't permitted. Thus, we must implement
/// this trait on a different type and take a [Network] as an argument, giving us just one mutable
/// reference to [Network] and the implementer's `self`.
trait Run<CT: ClientThread> {
    /// Sends a [NetworkControlMessage] to a client thread.
    ///
    /// If no thread exists for the specified host, spawns a new one and delivers the message.
    fn send(&mut self, network: &mut Network<CT>, message: NetworkControlMessage);

    /// Handles a client that has sent a disconnection message.
    ///
    /// # Design notes
    ///
    /// We use generics for `error` because the calling code can most easily and efficiently
    /// generate one `&Option<String>` and one `&Option<&String>`.
    fn disconnect_client<S: ToString>(
        &mut self,
        network: &mut Network<CT>,
        host: &str,
        error: &Option<S>,
    );
}

/// Implements [Run] for production use.
struct NetworkRun();

impl<CT: ClientThread> Run<CT> for NetworkRun {
    fn send(&mut self, network: &mut Network<CT>, message: NetworkControlMessage) {
        use NetworkControlMessage::*;
        let host = match &message {
            RunAction(host_action) => host_action.host().to_string(),
            Disconnect(host) => host.to_string(),
        };
        let client = network.client_for(&host);

        // Send the message to the client. If the sender can't send, that means the client thread
        // closed without sending a disconnection [Report]. This was either a panic due to a
        // detected bug or an exit without panic due to an undetected bug. Either way, we need to
        // panic appropriately.
        if client.outbox.send(message).is_err() {
            if client.thread.is_finished() {
                // We need to consume Client::thread, which we can't do from behind a reference.
                let client = network.connections.remove(&host).unwrap();
                let result = client.thread.join();
                match result {
                    Ok(_) => {
                        panic!("Network client thread exited mysteriously. Please report this bug!")
                    }
                    Err(err) => std::panic::resume_unwind(err),
                }
            } else {
                panic!(
                    "Network client thread closed its Receiver but appears to still be running. \
                    Please report this bug!"
                );
            }
        }
    }

    fn disconnect_client<S: ToString>(
        &mut self,
        network: &mut Network<CT>,
        host: &str,
        error: &Option<S>,
    ) {
        // Remove the client from connections.
        let client = network.connections.remove(host).unwrap();

        // Join the thread. This will block until the client has exited, which it should promptly
        // do after it sends a disconnection message of any kind.
        let result = client.thread.join();

        // Log the precise disconnection, since we don't [Report] it.
        match error {
            Some(error) => network.log.error(format!(
                "Client thread for {} exited with result {:?} and reported error {}",
                host,
                result,
                error.to_string(),
            )),
            None => network.log.notice(format!(
                "Client thread for {} exited with result {:?}",
                host, result,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::HostAction;
    use crate::core::fixtures::plan;
    use crate::executor;
    use std::sync::Arc;

    /// An implementation of [ClientThread] that does nothing.
    pub struct TestThread {
        host: String,
        sender: Sender<Report>,
        receiver: Receiver<NetworkControlMessage>,
    }

    impl ClientThread for TestThread {
        fn new(
            host: String,
            sender: Sender<Report>,
            receiver: Receiver<NetworkControlMessage>,
        ) -> Self {
            TestThread {
                host,
                sender,
                receiver,
            }
        }

        fn run(self) {}
    }
    /// Fake implementation of [Run] that simply logs calls.
    struct TestRun {
        /// Stores any [NetworkControlMessage]s from [Self::send].
        messages: Vec<NetworkControlMessage>,

        /// Stores the arguments of any calls to [Self::disconnect_client].
        disconnections: Vec<(String, Option<String>)>,
    }

    impl TestRun {
        /// Returns an empty [TestRun].
        fn new() -> Self {
            TestRun {
                messages: vec![],
                disconnections: vec![],
            }
        }
    }

    impl<CT: ClientThread> Run<CT> for TestRun {
        fn send(&mut self, _network: &mut Network<CT>, message: NetworkControlMessage) {
            self.messages.push(message);
        }

        fn disconnect_client<S: ToString>(
            &mut self,
            _network: &mut Network<CT>,
            host: &str,
            error: &Option<S>,
        ) {
            let host = host.to_string();
            let error = error.as_ref().map(|ok| ok.to_string());
            self.disconnections.push((host, error));
        }
    }

    /// Returns a test [Network] with all its connections to other parts of the system.
    ///
    /// Returns an [executor::ChannelPair] representing [executor], a [Receiver] for log messages,
    /// a `Network<TestThread>`, and a [TestRun] to pass to `_run_once` (if you need it).
    fn fixture() -> (
        executor::ChannelPair<NetworkControlMessage, Report>,
        Receiver<String>,
        Network<TestThread>,
        TestRun,
    ) {
        // Generate fixtures for Run and Log.
        let run = TestRun::new();
        let (log, log_receiver) = Log::fixture();

        let (ncm_send, ncm_receive) = channel::unbounded();
        let (report_send, report_receive) = channel::unbounded();

        // Our pretend executor.
        let executor = executor::ChannelPair {
            sender: ncm_send,
            receiver: report_receive,
        };

        // Passed to network for its use; paired to executor.
        let to_executor = ChannelPair {
            sender: report_send,
            receiver: ncm_receive,
        };

        let network: Network<TestThread> = Network::new(to_executor, log);

        (executor, log_receiver, network, run)
    }

    /// Returns a [NetworkControlMessage::RunAction].
    fn ncm_run_action() -> NetworkControlMessage {
        let (_, manifest, task, action) = plan();
        let host_action = HostAction::new(&manifest.hosts[0], &manifest, &task, &action);
        NetworkControlMessage::RunAction(Arc::new(host_action))
    }

    /// Generates a simple, innocuous [Report] that can safely be sent without triggering
    /// side effects.
    fn report() -> Report {
        Report::Connecting("fake_host".into())
    }

    mod _run_once {
        use super::*;

        mod with_inbox_message {
            use super::*;

            #[test]
            fn prioritizes_inbox_over_executor() {
                let (executor, _, mut network, mut run) = fixture();
                let report = report();
                let message = ncm_run_action();

                // Send messages to both Receivers. See which one gets processed.
                network.client_outbox.send(report.clone()).unwrap();
                executor.sender.send(message.clone()).unwrap();

                let _ = network._run_once(&mut run);

                // Verify that `network.inbox` gets processed.
                assert!(matches!(network.inbox.try_recv(), Err(TryRecvError::Empty)));
                assert_eq!(report, executor.receiver.try_recv().unwrap());

                // Verify that network.executor does not get processed.
                assert!(network.executor.receiver.try_recv().is_ok());
            }

            #[test]
            fn calls_disconnect_client_on_failed_to_connect() {
                let (_, _, mut network, mut run) = fixture();
                const HOST: &str = "help_im_under_dressed";
                const ERROR: &str = "didn't know it was a black-tie formal";

                network
                    .client_outbox
                    .send(Report::FailedToConnect {
                        host: HOST.into(),
                        error: ERROR.into(),
                    })
                    .unwrap();

                let _ = network._run_once(&mut run);

                assert_eq!(vec![(HOST.into(), Some(ERROR.into()))], run.disconnections);
            }

            #[test]
            fn calls_disconnect_client_on_disconnected() {
                let (_, _, mut network, mut run) = fixture();
                const HOST: &str = "disconnect_me";
                const ERROR: &str = "the server was too sunny";

                network
                    .client_outbox
                    .send(Report::Disconnected {
                        host: HOST.into(),
                        error: Some(ERROR.into()),
                    })
                    .unwrap();

                let _ = network._run_once(&mut run);

                assert_eq!(vec![(HOST.into(), Some(ERROR.into()))], run.disconnections);
            }

            #[test]
            fn reports_to_executor_and_returns_true_ok() {
                let (executor, _, mut network, mut run) = fixture();
                let report = report();

                network.client_outbox.send(report.clone()).unwrap();

                let retval = network._run_once(&mut run);

                assert_eq!(executor.receiver.try_recv().unwrap(), report);
                assert!(
                    matches!(retval, (true, Ok(()))),
                    "Expected (true, Ok(()) but got: {:?}",
                    retval,
                );
            }

            #[test]
            fn quits_without_error_if_executor_closed() {
                let (executor, _, mut network, mut run) = fixture();

                drop(executor.receiver);
                network.client_outbox.send(report()).unwrap();

                let retval = network._run_once(&mut run);

                assert!(
                    matches!(retval, (false, Ok(()))),
                    "Expected (false, Ok(()) but got: {:?}",
                    retval,
                );
            }

            // To the best of my knowledge, there isn't a good way to test TryRecvError::Empty.

            #[test]
            #[should_panic(expected = "All network client Senders disconnected")]
            fn panics_if_inbox_closed() {
                let (_, _, mut network, mut run) = fixture();

                // Swap out `network.client_outbox` and drop it. Should be impossible in
                // production environments, as no code path should ever do this.
                let (sender, _) = channel::unbounded();
                let client_outbox = std::mem::replace(&mut network.client_outbox, sender);
                drop(client_outbox);

                // Now `network.inbox` has no open Senders, so it should return Disconnected.
                let _ = network._run_once(&mut run);
            }
        }

        mod with_executor_message {
            use super::*;

            #[test]
            fn calls_send() {
                let (executor, _, mut network, mut run) = fixture();

                let message = ncm_run_action();
                executor.sender.send(message.clone()).unwrap();
                let _ = network._run_once(&mut run);

                assert_eq!(vec![message], run.messages);
            }

            // To the best of my knowledge, there isn't a good way to test TryRecvError::Empty.

            #[test]
            fn quits_without_error_if_executor_closed() {
                let (executor, _, mut network, mut run) = fixture();

                // Explicitly close executor.
                drop(executor);

                // _run_once should find that its executor channel is ready, and it should read an
                // Err(_) indicating that executor is done and it's time to quit.

                let retval = network._run_once(&mut run);

                assert!(
                    matches!(retval, (false, Ok(()))),
                    "Expected (false, Ok(()) but got: {:?}",
                    retval,
                );
            }
        }

        #[test]
        fn returns_true_ok() {
            let (_executor, _, mut network, mut run) = fixture();

            let retval = network._run_once(&mut run);

            assert!(
                matches!(retval, (true, Ok(()))),
                "Expected (true, Ok(()) but got: {:?}",
                retval,
            );
        }
    }
}
