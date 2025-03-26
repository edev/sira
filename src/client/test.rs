use super::*;
use std::fs;
use std::io::{self, Write};

mod mktemp {
    use super::*;

    // We don't attempt to test the documented error conditions, because they should all be nearly
    // impossible in any sane environment, and these tests don't set up custom environments such as
    // Docker containers.

    #[test]
    fn file_and_path_match() -> anyhow::Result<()> {
        let test_string = "file and path must match";
        let (mut file, path) = mktemp()?;
        file.write_all(test_string.as_bytes())?;
        drop(file);
        let contents = fs::read_to_string(path)?;
        assert_eq!(contents, test_string);
        Ok(())
    }
}

mod run {
    use super::*;

    #[test]
    fn empty_cmd_or_failure_to_start() {
        let error = run("", &["a", "b", "c"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to start command: '' a b c")
        );
        let error: io::Error = error.downcast().unwrap();
        assert_eq!(io::ErrorKind::NotFound, error.kind());
    }

    #[test]
    fn empty_args() -> anyhow::Result<()> {
        run::<&str, &str>("echo", &[])
    }

    #[test]
    fn exit_failure() {
        let error = run("bash", &["-c", "false"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("command exited with exit code 1: bash -c false")
        );
    }

    #[test]
    fn exit_success() -> anyhow::Result<()> {
        run("bash", &["-c", "true"])
    }

    #[test]
    fn command_representation() {
        // Test cases, in format: (cmd, args, expected_output). All cases must generate error
        // messages, or there will be no output to compare.
        //
        // We deliberately don't go into much detail, here, because we don't want to depend on
        // shlex's behavior in our tests any more than necessary.
        let cases = [
            // Components that don't need modification are unmodified.
            ("bash", ["-c", "false"], "bash -c false"),
            // Components that contain spaces are quoted.
            (
                "bash",
                ["-c", "echo -n && false"],
                "bash -c 'echo -n && false'",
            ),
        ];
        for (cmd, args, expected) in cases {
            let error = run(cmd, &args).unwrap_err();
            assert!(
                error.to_string().contains(expected),
                "error did not contain expected string:\nExpected: {expected}\nActual: {error}",
            );
        }
    }
}

mod whoami {
    use super::*;

    #[test]
    fn users_contains_whoami() {
        let users = {
            let output = Command::new("users").output().unwrap();
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            String::from_utf8(output.stdout).unwrap()
        };
        assert!(users.contains(whoami()));
    }

    #[test]
    fn trimmed() {
        assert_eq!(whoami(), whoami().trim());
    }
}
