use anyhow::{anyhow, bail};
use shlex::Shlex;
use sira::core::Action;
use sira::crypto;
use std::env;
use std::fs;
use std::process::Command;

/// The name of the allowed signers file used to verify actions.
pub const ALLOWED_SIGNERS_FILE: &str = "action";

fn main() -> anyhow::Result<()> {
    // Number of actual arguments (excluding the name of the binary).
    let argc = env::args()
        .len()
        .checked_sub(1)
        .expect("integer underflow in argc-1; please report this");

    let require_signature = crypto::allowed_signers_installed(ALLOWED_SIGNERS_FILE)?;

    if argc == 2 && !require_signature {
        // Error: missing allowed signers file.
        return error_missing_allowed_signers_file();
    } else if argc == 1 && require_signature {
        // Error: missing signature.
        //
        // If the administrator is trying to run an action directly, this error will be misleading,
        // but that's not an intended use case. As of this writing, we don't have a good way to
        // differentiate these cases.
        return error_missing_signature();
    } else if argc == 0 || argc > 2 {
        // Error: too few or too many arguments.
        //
        // The only reason this code path should ever run is that the administrator tried to run an
        // action on a client manually. This isn't an intended use case, but we'll provide more
        // detailed instructions here than in the other cases in order to help the administrator on
        // their way.
        return error_wrong_arguments(require_signature);
    }

    let yaml = env::args().nth(1).expect(
        "missing required argument <action-as-yaml>, but failed to detect this and display a \
        helpful error message to the user",
    );

    if require_signature {
        let signature = env::args().nth(2).expect(
            "missing required argument <action-signature>, but failed to detect this and display a \
            helpful error message to the user",
        );

        // TODO Implement a more permanent and secure version of this! This is only temporary, for
        // (pre-)alpha development!
        let signature_path: &str = ".sira-signature";
        fs::write(signature_path, signature)?;

        crypto::verify(yaml.as_bytes(), signature_path, ALLOWED_SIGNERS_FILE, "sira")?;
    }

    let action: Action = serde_yaml::from_str(&yaml)?;

    use Action::*;
    match action {
        Shell(commands) => {
            for command_string in commands {
                let mut words = Shlex::new(&command_string);
                let command = words
                    .next()
                    .ok_or(anyhow!("sira-client received a blank shell command"))?;
                let args: Vec<_> = words.collect();
                let child_exit_status = Command::new(command).args(&args).spawn()?.wait()?;
                if !child_exit_status.success() {
                    let exit_code_message = match child_exit_status.code() {
                        Some(i) => format!("exit code {i}"),
                        None => "error".to_string(),
                    };
                    bail!("command exited with {exit_code_message}: {command_string}");
                }
            }
        }
        LineInFile { .. } => bail!("not yet implemented"),
        Upload { .. } | Download { .. } => {
            bail!("this action is implemented on sira, not sira-client");
        }
    }
    Ok(())
}

fn error_missing_allowed_signers_file() -> anyhow::Result<()> {
    bail!(
        "Please install the action allowed signers file:\n\
        \n\
        {}\n",
        crypto::allowed_signers_path(ALLOWED_SIGNERS_FILE)?.to_string_lossy(),
    );
}

fn error_missing_signature() -> anyhow::Result<()> {
    bail!(
        "Missing signature. Please install the action private key on the control node:\n\
        \n\
        {}\n",
        crypto::allowed_signers_path(sira::run_plan::ACTION_SIGNING_KEY)?.to_string_lossy(),
    )
}

fn error_wrong_arguments(require_signature: bool) -> anyhow::Result<()> {
    bail!(
        "\
Please provide the correct arguments:

    sira-client <action-as-yaml> [<action-signature>]

The first argument is an Action written in YAML format.

The second argument is a cryptographic signature for this action, generated by invoking \
`ssh-keygen -Y sign` on the action. This is required if the allowed signers file is \
installed:

    Location: {}
    Status: {}\n\
        ",
        crypto::allowed_signers_path(ALLOWED_SIGNERS_FILE)?.to_string_lossy(),
        match require_signature {
            true => "Installed",
            false => "Not installed",
        }
    );
}
