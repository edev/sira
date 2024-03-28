//! Provides a [tokio]-based [Plan] runner that runs on each host in parallel.

use crate::core::plan::HostPlanIntoIter;
use crate::core::Plan;

mod client;
use client::*;

mod report;
use report::*;

/// Runs a [Plan] on each of the [Plan]'s hosts in parallel.
///
/// If a host is unreachable, it will simply be skipped; the [Plan] will still run to completion on
/// every available host.
///
/// Similarly, if a host encounters an error, either due to a connection issue or an [Action] that
/// fails (e.g. an [Action::Shell] that returns a non-zero exit code), that host will execute no
/// further [Action]s, but other hosts will run to completion.
///
/// # Returns
///
/// On success, returns `Ok(())`. On failure, returns a list of failure tuples of the form
/// `(host, error)`. Hosts that do not appear in this error return have successfully run their
/// portion of the [Plan].
///
/// [Action]: crate::core::Action
/// [Action::Shell]: crate::core::Action::Shell
pub async fn run_plan(plan: Plan) -> Result<(), Vec<(String, anyhow::Error)>> {
    _run_plan(plan, ConnectionManager, Reporter).await
}

/// Provides dependency injection for unit-testing [run_plan] without SSH, stdout, or stderr.
async fn _run_plan<C: ClientInterface, CM: ManageClient<C> + Clone, R: Report + Clone>(
    plan: Plan,
    connection_manager: CM,
    reporter: R,
) -> Result<(), Vec<(String, anyhow::Error)>> {
    // Holds tuples of (host, future) where future is the future for the async task that's running
    // the plan on host. We want to spawn all tasks without awaiting and store the futures so we
    // can await them all afterward.
    let mut run_futures = Vec::new();

    for host in plan.hosts() {
        let host_plan = plan.plan_for(&host).unwrap().into_iter();
        run_futures.push((
            host.clone(),
            run_host_plan(
                host,
                host_plan,
                connection_manager.clone(),
                reporter.clone(),
            ),
        ));
    }

    let mut errors = Vec::new();
    for (host, future) in run_futures {
        if let Err(err) = future.await {
            errors.push((host, err));
        }
    }

    match errors.len() {
        0 => Ok(()),
        _ => Err(errors),
    }
}

/// Runs a [Plan] on a single host via [HostPlanIntoIter].
async fn run_host_plan<C: ClientInterface, CM: ManageClient<C>, R: Report + Clone>(
    host: String,
    plan: HostPlanIntoIter,
    mut connection_manager: CM,
    mut reporter: R,
) -> anyhow::Result<()> {
    let mut client = connection_manager.connect(&host).await?;

    for action in plan {
        let action = action.compile();
        let yaml = serde_yaml::to_string(&action).unwrap();

        use crate::core::Action::*;
        let output = match &action {
            Shell(_) => client.shell(&yaml).await?,
            LineInFile { .. } => client.line_in_file(&yaml).await?,
            Upload { from, to } => client.upload(from, to).await?,
            Download { from, to } => client.download(from, to).await?,
        };

        reporter.report(&host, yaml, &output).await?;

        if !output.status.success() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixtures::plan;
    use crate::core::Action;
    use anyhow::bail;
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
    use std::error::Error;
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};
    use std::sync::{Arc, Mutex, MutexGuard};

    pub mod fixtures {
        use super::*;

        pub mod fixture {
            use super::*;

            pub struct Fixture {
                pub host: String,
                pub plan: Plan,
                pub client_factory: Arc<Mutex<TestClientFactory>>,
                pub reporter: Arc<TestReporter>,
            }

            impl Fixture {
                pub fn new() -> Self {
                    let (plan, mut manifest, _, _) = plan();
                    let host = manifest.hosts.pop().unwrap();
                    let client_factory = TestClientFactory::new();
                    let reporter = TestReporter::new();

                    Fixture {
                        host,
                        plan,
                        client_factory,
                        reporter,
                    }
                }

                // Returns a locked and modifiable TestClientFactory.
                pub fn client_factory(&self) -> MutexGuard<'_, TestClientFactory> {
                    self.client_factory.lock().unwrap()
                }

                // Calls run_host_plan, passing in Fixture's fields.
                pub async fn run_host_plan(&self) -> anyhow::Result<()> {
                    run_host_plan(
                        self.host.clone(),
                        self.plan.plan_for(&self.host).unwrap().into_iter(),
                        self.client_factory.clone(),
                        self.reporter.clone(),
                    )
                    .await
                }

                // Consumes Self and returns the record of client commands for examination.
                pub fn recorded_commands(self) -> Vec<CommandRecord> {
                    // Unpack the Arc<Mutex<TestClientFactory>> into a Mutex<_>.
                    let cf_mutex = Arc::into_inner(self.client_factory).unwrap();

                    // Unpack the Mutex<ClientFactory> into a ClientFactory.
                    let cf = cf_mutex.into_inner().unwrap();

                    // Unpack the ClientFactory into a ClientCommands, which is a HashMap, and retrieve
                    // the Arc<Mutex<Vec<CommandRecord>>> for self.host.
                    let cr_arc_mutex = cf.into_client_commands().remove(&self.host).unwrap();

                    // Unpack the Arc<Mutex<Vec<CommandRecord>>> into a Mutex.
                    let cr_mutex = Arc::into_inner(cr_arc_mutex).unwrap();

                    // Unpack the Mutex and return.
                    cr_mutex.into_inner().unwrap()
                }

                // DRY helper for run_host_plan tests that verify the mapping between an Action
                // enum variant and a ClientInterface method.
                pub async fn test_calls_client(method_name: &'static str, action: Action) {
                    let mut fixture = Fixture::new();
                    let yaml = serde_yaml::to_string(&action).unwrap();
                    fixture.plan.manifests[0].include[0].actions = vec![action];

                    fixture.run_host_plan().await.unwrap();

                    let recorded_commands = fixture.recorded_commands();
                    let expected = CommandRecord { method_name, yaml };
                    assert_eq!(expected, recorded_commands[0]);
                }

                // DRY helper for run_host_plan tests that verify the error handling on
                // ClientInterface methods.
                pub async fn test_client_returns_error(method_name: &'static str, action: Action) {
                    let mut fixture = Fixture::new();
                    let yaml = serde_yaml::to_string(&action).unwrap();
                    fixture.plan.manifests[0].include[0].actions = vec![action];
                    fixture.client_factory().fail_client_command(&fixture.host);

                    assert!(fixture.run_host_plan().await.is_err());

                    let recorded_commands = fixture.recorded_commands();
                    let expected = CommandRecord { method_name, yaml };
                    assert_eq!(expected, recorded_commands[0]);
                }
            }
        }
        pub use fixture::*;

        pub mod client {
            use super::*;

            // A factory that tests can pass into _run_plan. Spawns TestClient values when asked to
            // connect to clients, but holds onto references to the TestClient's fields so that
            // tests can examine them later by inspecting the TestClientFactory.
            #[derive(Clone, Debug)]
            pub struct TestClientFactory {
                // A record of all ClientInterface methods invoked on all clients.
                client_commands: ClientCommands,

                // Clients that the connect method should not be able to reach.
                unreachable_clients: HashSet<String>,

                // Clients whose ClientInterface methods should return failures.
                failing_clients: HashSet<String>,

                // Maps host_name -> exit_code. Allows clients to return custom exit codes via
                // ClientInterface to simulate failed commands.
                custom_exit_codes: HashMap<String, i32>,
            }

            impl TestClientFactory {
                pub fn new() -> Arc<Mutex<Self>> {
                    Arc::new(Mutex::new(Self {
                        client_commands: ClientCommands::new(),
                        unreachable_clients: HashSet::new(),
                        failing_clients: HashSet::new(),
                        custom_exit_codes: HashMap::new(),
                    }))
                }

                pub fn set_unreachable(&mut self, host: impl Into<String>) {
                    self.unreachable_clients.insert(host.into());
                }

                pub fn fail_client_command(&mut self, host: impl Into<String>) {
                    self.failing_clients.insert(host.into());
                }

                pub fn exit_code(&mut self, host: impl Into<String>, code: i32) {
                    self.custom_exit_codes.insert(host.into(), code);
                }

                pub fn client_commands(&self) -> &ClientCommands {
                    &self.client_commands
                }

                pub fn into_client_commands(self) -> ClientCommands {
                    self.client_commands
                }
            }

            #[async_trait]
            impl ManageClient<TestClient> for Arc<Mutex<TestClientFactory>> {
                async fn connect(&mut self, host: &str) -> anyhow::Result<TestClient> {
                    let mut factory = self.lock().unwrap();
                    if factory.unreachable_clients.contains(host) {
                        bail!("unreachable");
                    }

                    use std::collections::hash_map::Entry;
                    let commands = match factory.client_commands.entry(host.to_owned()) {
                        Entry::Occupied(entry) => entry.get().clone(),
                        Entry::Vacant(entry) => entry.insert(Arc::default()).clone(),
                    };

                    let should_fail = factory.failing_clients.contains(host);

                    let custom_exit_code = factory.custom_exit_codes.get(host).copied();

                    Ok(TestClient {
                        records: commands,
                        should_fail,
                        custom_exit_code,
                    })
                }
            }

            // Maps host_name -> SharedRecords.
            type ClientCommands = HashMap<String, SharedRecords>;

            // The record of ClientInterface method calls for a single TestClient.
            type SharedRecords = Arc<Mutex<Vec<CommandRecord>>>;

            #[derive(Clone, Debug, PartialEq, Eq)]
            pub struct CommandRecord {
                pub method_name: &'static str,
                pub yaml: String,
            }

            #[derive(Clone, Debug)]
            pub struct TestClient {
                // ClientInterface methods invoked on this client.
                records: SharedRecords,

                // Whether ClientInterface methods should return Err values.
                should_fail: bool,

                // Optional custom value that a ClientInterface method should return on success as
                // part of its Output value.
                custom_exit_code: Option<i32>,
            }

            #[async_trait]
            impl ClientInterface for TestClient {
                async fn shell(&mut self, yaml: &str) -> Result<Output, openssh::Error> {
                    self.record("shell", yaml, openssh::Error::Disconnected)
                }

                async fn line_in_file(&mut self, yaml: &str) -> Result<Output, openssh::Error> {
                    self.record("line_in_file", yaml, openssh::Error::Disconnected)
                }

                async fn upload(&mut self, from: &str, to: &str) -> io::Result<Output> {
                    let action = Action::Upload {
                        from: from.to_owned(),
                        to: to.to_owned(),
                    };
                    let yaml = serde_yaml::to_string(&action).unwrap();
                    self.record("upload", yaml, io::Error::other("expected"))
                }

                async fn download(&mut self, from: &str, to: &str) -> io::Result<Output> {
                    let action = Action::Download {
                        from: from.to_owned(),
                        to: to.to_owned(),
                    };
                    let yaml = serde_yaml::to_string(&action).unwrap();
                    self.record("download", yaml, io::Error::other("expected"))
                }
            }

            impl TestClient {
                // Records a call to a ClientInterface method.
                pub fn record<E: Error>(
                    &mut self,
                    caller: &'static str,
                    yaml: impl Into<String>,
                    error: E,
                ) -> Result<Output, E> {
                    self.records.lock().unwrap().push(CommandRecord {
                        method_name: caller,
                        yaml: yaml.into(),
                    });

                    if self.should_fail {
                        Err(error)
                    } else {
                        let exit_code = self.custom_exit_code.unwrap_or(0);
                        Ok(Output {
                            status: ExitStatus::from_raw(exit_code),
                            stdout: vec![],
                            stderr: vec![],
                        })
                    }
                }
            }
        }
        pub use client::*;

        pub mod report {
            use super::*;

            // A Report implementation that uses Vecs as fake stdout/stderr writers. _run_plan
            // needs to clone its reporter for each client, so TestReporter uses shared references
            // to the same stdout and stderr Vecs, and tests can hold onto a clone and examine it
            // after the code under test.
            #[derive(Debug, Default)]
            pub struct TestReporter {
                // The shared stdout writer.
                stdout: Mutex<Vec<u8>>,

                // The shared stderr writer.
                stderr: Mutex<Vec<u8>>,

                // Whether the Report::report method should return an error.
                should_fail: Mutex<bool>,
            }

            impl TestReporter {
                pub fn new() -> Arc<Self> {
                    Arc::new(Self {
                        stdout: Mutex::new(vec![]),
                        stderr: Mutex::new(vec![]),
                        should_fail: Mutex::new(false),
                    })
                }

                pub fn stdout(&self) -> MutexGuard<Vec<u8>> {
                    self.stdout.lock().unwrap()
                }

                // Instructs this TestReporter to always fail, i.e. return an error from the
                // Report::report method.
                pub fn fail(&self) {
                    *self.should_fail.lock().unwrap() = true;
                }
            }

            #[async_trait]
            impl Report for Arc<TestReporter> {
                // Performs a simulated report, and then optionally returns an expected failure.
                async fn report(
                    &mut self,
                    host: &str,
                    yaml: String,
                    output: &Output,
                ) -> io::Result<()> {
                    let result = _report(
                        self.stdout.lock().unwrap(),
                        self.stderr.lock().unwrap(),
                        host,
                        yaml,
                        output,
                    );

                    if *self.should_fail.lock().unwrap() {
                        Err(io::Error::other("expected"))
                    } else {
                        result
                    }
                }
            }
        }
        pub use report::*;
    }
    use fixtures::*;

    mod _run_plan {
        use super::*;

        #[tokio::test]
        async fn runs_plan_for_all_hosts() {
            let mut fixture = Fixture::new();
            fixture.plan.manifests[0].hosts =
                vec!["a".to_string(), "b".to_string(), "c".to_string()];

            _run_plan(
                fixture.plan.clone(),
                fixture.client_factory.clone(),
                fixture.reporter.clone(),
            )
            .await
            .unwrap();

            let locked = fixture.client_factory();
            assert_eq!(3, locked.client_commands().len());
            assert!(locked.client_commands().contains_key("a"));
            assert!(locked.client_commands().contains_key("b"));
            assert!(locked.client_commands().contains_key("c"));
        }

        #[tokio::test]
        async fn returns_all_errors() {
            let mut fixture = Fixture::new();
            fixture.plan.manifests[0].hosts =
                vec!["a".to_string(), "b".to_string(), "c".to_string()];

            {
                let mut locked = fixture.client_factory();
                locked.fail_client_command("b");
                locked.fail_client_command("c");
            }

            let errors = _run_plan(
                fixture.plan.clone(),
                fixture.client_factory.clone(),
                fixture.reporter.clone(),
            )
            .await
            .unwrap_err();

            assert_eq!(2, errors.len());
        }

        #[tokio::test]
        async fn returns_ok() {
            let mut fixture = Fixture::new();
            fixture.plan.manifests[0].hosts =
                vec!["a".to_string(), "b".to_string(), "c".to_string()];
            assert!(_run_plan(
                fixture.plan.clone(),
                fixture.client_factory.clone(),
                fixture.reporter.clone(),
            )
            .await
            .is_ok());
        }
    }

    mod run_host_plan {
        use super::*;

        #[tokio::test]
        async fn connects_to_host() {
            let fixture = Fixture::new();
            fixture.run_host_plan().await.unwrap();
            assert_eq!(1, fixture.client_factory().client_commands().len());
        }

        #[tokio::test]
        async fn returns_error_if_fails_to_connect() {
            let fixture = Fixture::new();
            fixture.client_factory().set_unreachable(&fixture.host);
            assert!(fixture.run_host_plan().await.is_err());
        }

        #[tokio::test]
        async fn runs_all_actions_in_host_plan() {
            let mut fixture = Fixture::new();

            // As of writing, Action::Shell processes through this code as a single Action, even if
            // there are multiple commands in the Shell's Vec, and the crate::core::fixtures::plan
            // method provides one Action::Shell with two commands. Thus, for this one test where
            // it matters, we provide our own Actions.
            fixture.plan.manifests[0].include[0].actions = vec![
                Action::Download {
                    from: "a".to_string(),
                    to: "b".to_string(),
                },
                Action::Upload {
                    from: "c".to_string(),
                    to: "d".to_string(),
                },
            ];

            fixture.run_host_plan().await.unwrap();

            // Obtain Mutex locks and generate an iterator over recorded commands from the run.
            let locked_client_factory = fixture.client_factory();
            let locked_client_commands = locked_client_factory.client_commands()[&fixture.host]
                .lock()
                .unwrap();
            let mut commands = locked_client_commands.iter();

            assert_eq!(
                &CommandRecord {
                    method_name: "download",
                    yaml: serde_yaml::to_string(&fixture.plan.manifests[0].include[0].actions[0])
                        .unwrap(),
                },
                commands.next().unwrap(),
            );
            assert_eq!(
                &CommandRecord {
                    method_name: "upload",
                    yaml: serde_yaml::to_string(&fixture.plan.manifests[0].include[0].actions[1])
                        .unwrap(),
                },
                commands.next().unwrap(),
            );
        }

        mod shell {
            use super::*;

            #[tokio::test]
            async fn calls_client_shell() {
                Fixture::test_calls_client("shell", Action::Shell(vec!["send_it".to_string()]))
                    .await
            }

            #[tokio::test]
            async fn returns_error_on_failure() {
                Fixture::test_client_returns_error(
                    "shell",
                    Action::Shell(vec!["send_it".to_string()]),
                )
                .await
            }
        }

        mod line_in_file {
            use super::*;

            #[tokio::test]
            async fn calls_client_line_in_file() {
                Fixture::test_calls_client(
                    "line_in_file",
                    Action::LineInFile {
                        path: "a".to_string(),
                        line: "b".to_string(),
                        pattern: None,
                        after: None,
                        indent: true,
                    },
                )
                .await
            }

            #[tokio::test]
            async fn returns_error_on_failure() {
                Fixture::test_client_returns_error(
                    "line_in_file",
                    Action::LineInFile {
                        path: "a".to_string(),
                        line: "b".to_string(),
                        pattern: None,
                        after: None,
                        indent: true,
                    },
                )
                .await
            }
        }

        mod upload {
            use super::*;

            #[tokio::test]
            async fn calls_client_upload() {
                Fixture::test_calls_client(
                    "upload",
                    Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                    },
                )
                .await
            }

            #[tokio::test]
            async fn returns_error_on_failure() {
                Fixture::test_client_returns_error(
                    "upload",
                    Action::Upload {
                        from: "a".to_string(),
                        to: "b".to_string(),
                    },
                )
                .await
            }
        }

        mod download {
            use super::*;

            #[tokio::test]
            async fn calls_client_download() {
                Fixture::test_calls_client(
                    "download",
                    Action::Download {
                        from: "a".to_string(),
                        to: "b".to_string(),
                    },
                )
                .await
            }

            #[tokio::test]
            async fn returns_error_on_failure() {
                Fixture::test_client_returns_error(
                    "download",
                    Action::Download {
                        from: "a".to_string(),
                        to: "b".to_string(),
                    },
                )
                .await
            }
        }

        #[tokio::test]
        async fn reports_action() {
            let fixture = Fixture::new();
            fixture.run_host_plan().await.unwrap();
            assert!(String::from_utf8(fixture.reporter.stdout().to_vec())
                .unwrap()
                .contains("Ran action on"));
        }

        #[tokio::test]
        async fn returns_error_if_reporting_fails() {
            let fixture = Fixture::new();
            fixture.reporter.fail();
            assert!(fixture.run_host_plan().await.is_err());
        }

        #[tokio::test]
        async fn returns_if_action_fails() {
            let mut fixture = Fixture::new();
            fixture.plan.manifests[0].include[0].actions = vec![
                Action::Upload {
                    from: "a".to_string(),
                    to: "b".to_string(),
                },
                Action::Download {
                    from: "c".to_string(),
                    to: "d".to_string(),
                },
            ];
            fixture.client_factory().exit_code(&fixture.host, -1);

            fixture.run_host_plan().await.unwrap();

            let recorded_commands = fixture.recorded_commands();
            assert_eq!(1, recorded_commands.len());
        }

        #[tokio::test]
        async fn returns_ok() {
            assert!(Fixture::new().run_host_plan().await.is_ok());
        }
    }
}