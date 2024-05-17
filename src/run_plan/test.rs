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

    // If `signed`, tries to sign `yaml` and returns the result of that attempt. If the
    // signing key isn't found (which is not an error condition), or `signed` is `false`,
    // returns `None`.
    pub fn maybe_sign(yaml: &str, signed: bool) -> Option<String> {
        match signed {
            true => match crypto::sign(yaml.as_bytes(), ACTION_SIGNING_KEY).unwrap() {
                SigningOutcome::Signed(sig) => {
                    Some(String::from_utf8(sig).expect("signature was not UTF-8"))
                }
                SigningOutcome::KeyNotFound => None,
            },
            false => None,
        }
    }

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
            pub async fn test_calls_client(
                method_name: &'static str,
                action: Action,
                signed: bool,
            ) {
                let mut fixture = Fixture::new();
                let yaml = serde_yaml::to_string(&action).unwrap();
                fixture.plan.manifests[0].include[0].actions = vec![action];
                let signature = maybe_sign(&yaml, signed);

                fixture.run_host_plan().await.unwrap();

                let recorded_commands = fixture.recorded_commands();
                let expected = CommandRecord {
                    method_name,
                    yaml,
                    signature,
                };
                assert_eq!(expected, recorded_commands[0]);
            }

            // DRY helper for run_host_plan tests that verify the error handling on
            // ClientInterface methods.
            pub async fn test_client_returns_error(
                method_name: &'static str,
                action: Action,
                signed: bool,
            ) {
                let mut fixture = Fixture::new();
                let yaml = serde_yaml::to_string(&action).unwrap();
                fixture.plan.manifests[0].include[0].actions = vec![action];
                fixture.client_factory().fail_client_command(&fixture.host);
                let signature = maybe_sign(&yaml, signed);

                assert!(fixture.run_host_plan().await.is_err());

                let recorded_commands = fixture.recorded_commands();
                let expected = CommandRecord {
                    method_name,
                    yaml,
                    signature,
                };
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
            pub signature: Option<String>,
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
            async fn command(
                &mut self,
                yaml: &str,
                signature: Option<Vec<u8>>,
            ) -> Result<Output, openssh::Error> {
                self.record("command", yaml, signature, openssh::Error::Disconnected)
            }

            async fn line_in_file(
                &mut self,
                yaml: &str,
                signature: Option<Vec<u8>>,
            ) -> Result<Output, openssh::Error> {
                self.record(
                    "line_in_file",
                    yaml,
                    signature,
                    openssh::Error::Disconnected,
                )
            }

            async fn script(
                &mut self,
                yaml: &str,
                signature: Option<Vec<u8>>,
            ) -> Result<Output, openssh::Error> {
                self.record("script", yaml, signature, openssh::Error::Disconnected)
            }

            async fn upload(
                &mut self,
                from: &str,
                yaml: &str,
                signature: Option<Vec<u8>>,
            ) -> anyhow::Result<Output> {
                // Sanity check.
                let action: Action = serde_yaml::from_str(yaml).unwrap();
                match action {
                    Action::Upload { from: af, .. } => assert_eq!(from, af),
                    x => panic!("expected Action::Upload but got:\n{x:#?}"),
                }

                // anyhow::Error doesn't implement std::error::Error. Meanwhile, upload returns
                // an anyhow::Result, and record requires and returns Error/Result. To solve
                // this incompatibility, we have to map_err. The error output from rustc isn't
                // very helpful on this issue.
                self.record("upload", yaml, signature, io::Error::other("expected"))
                    .map_err(Into::into)
            }
        }

        impl TestClient {
            // Records a call to a ClientInterface method.
            pub fn record<E: Error + Send + Sync + 'static>(
                &mut self,
                caller: &'static str,
                yaml: impl Into<String>,
                signature: Option<Vec<u8>>,
                error: E,
            ) -> Result<Output, E> {
                self.records.lock().unwrap().push(CommandRecord {
                    method_name: caller,
                    yaml: yaml.into(),
                    signature: signature
                        .map(|s| String::from_utf8(s).expect("signature was not UTF-8")),
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
            async fn starting(&mut self, _host: &str, _action: &Action) -> io::Result<()> {
                // TODO Test this if we keep it.
                Ok(())
            }

            // Performs a simulated report, and then optionally returns an expected failure.
            async fn report(
                &mut self,
                host: &str,
                action: &Action,
                output: &Output,
            ) -> io::Result<()> {
                let result = _report(
                    self.stdout.lock().unwrap(),
                    self.stderr.lock().unwrap(),
                    host,
                    action,
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
        fixture.plan.manifests[0].hosts = vec!["a".to_string(), "b".to_string(), "c".to_string()];

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
        fixture.plan.manifests[0].hosts = vec!["a".to_string(), "b".to_string(), "c".to_string()];

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
        fixture.plan.manifests[0].hosts = vec!["a".to_string(), "b".to_string(), "c".to_string()];
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

        // For simplicity, we provide our own, unsigned Actions instead of using the two
        // Action::Command values that ship with Fixture.
        fixture.plan.manifests[0].include[0].actions = vec![
            Action::Command(vec!["one".to_string()]),
            Action::Command(vec!["two".to_string()]),
        ];

        fixture.run_host_plan().await.unwrap();

        // Obtain Mutex locks and generate an iterator over recorded commands from the run.
        let locked_client_factory = fixture.client_factory();
        let locked_client_commands = locked_client_factory.client_commands()[&fixture.host]
            .lock()
            .unwrap();
        let mut commands = locked_client_commands.iter();

        let yaml = serde_yaml::to_string(&fixture.plan.manifests[0].include[0].actions[0]).unwrap();
        let signature = maybe_sign(&yaml, true);
        assert_eq!(
            &CommandRecord {
                method_name: "command",
                yaml,
                signature,
            },
            commands.next().unwrap(),
        );

        let yaml = serde_yaml::to_string(&fixture.plan.manifests[0].include[0].actions[1]).unwrap();
        let signature = maybe_sign(&yaml, true);
        assert_eq!(
            &CommandRecord {
                method_name: "command",
                yaml,
                signature,
            },
            commands.next().unwrap(),
        );
    }

    mod command {
        use super::*;

        #[tokio::test]
        async fn calls_client_command() {
            Fixture::test_calls_client(
                "command",
                Action::Command(vec!["send_it".to_string()]),
                true,
            )
            .await
        }

        #[tokio::test]
        async fn returns_error_on_failure() {
            Fixture::test_client_returns_error(
                "command",
                Action::Command(vec!["send_it".to_string()]),
                true,
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
                true,
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
                true,
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
                    user: "c".to_string(),
                    group: "d".to_string(),
                    permissions: Some("e".to_string()),
                    overwrite: true,
                },
                true,
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
                    user: "c".to_string(),
                    group: "d".to_string(),
                    permissions: Some("e".to_string()),
                    overwrite: true,
                },
                true,
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
            .contains("Completed "));
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
        fixture.plan.manifests[0].include[0].actions = vec![Action::Upload {
            from: "a".to_string(),
            to: "b".to_string(),
            user: "c".to_string(),
            group: "d".to_string(),
            permissions: Some("e".to_string()),
            overwrite: true,
        }];
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
