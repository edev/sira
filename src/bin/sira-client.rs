use anyhow::{anyhow, bail, Context};
use shlex::Shlex;
use sira::client;
use sira::core::action::{line_in_file, script, Action, FILE_TRANSFER_PATH};
use sira::crypto;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::Path;

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

        // Write the signature to a secure temporary file so we can pass it to ssh-keygen.
        let (mut signature_file, signature_path) = client::mktemp()?;
        signature_file
            .write_all(signature.as_bytes())
            .context("sira-client encountered an error writing action signature to disk")?;
        drop(signature_file);

        crypto::verify(
            yaml.as_bytes(),
            &signature_path,
            ALLOWED_SIGNERS_FILE,
            "sira",
        )?;

        fs::remove_file(&signature_path)
            .context("sira-client encountered an error removing action signature file")?;
    }

    let action: Action = serde_yaml::from_str(&yaml)?;

    match action {
        Action::Command(commands) => {
            for command_string in commands {
                let mut words = Shlex::new(&command_string);
                let command = words
                    .next()
                    .ok_or(anyhow!("sira-client received a blank command"))?;
                let args: Vec<_> = words.collect();
                client::run(command, &args)?;
            }
        }
        Action::LineInFile { .. } => line_in_file(&action)?,
        Action::Script { .. } => script(&action)?,
        Action::Upload {
            from,
            to,
            user,
            group,
            permissions,
            overwrite,
        } => {
            // It probably isn't exploitable, but let's try to perform some basic sanity checking
            // before we inject `{user}:{group}` into an argument and pass it to chown as root!
            //
            // We don't need to check for spaces, because Command already protects us from
            // argument-delimiter injection attacks by its use of a builder pattern.
            if user.contains(':') {
                bail!("user should not contain a colon (\":\") character: {user}");
            } else if group.contains(':') {
                bail!("group should not contain a colon (\":\") character: {group}");
            }

            // chmod the temporary file to its final state.
            //
            // We do this before we chown under the theory that it might be slightly more secure.
            // If the final permissions are more restrictive than the Sira user's default
            // permissions, then we don't want to change the user or group, thereby granting
            // additional potential access, before we restrict permissions for said user and group.
            if let Some(permissions) = permissions {
                client::run("chmod", &[&permissions[..], FILE_TRANSFER_PATH])?;
            }

            // chown the temporary file to its final state.
            client::run(
                "chown",
                &[&format!("{user}:{group}")[..], FILE_TRANSFER_PATH],
            )?;

            // Install the file, i.e. mv the file into place.
            let mut args: Vec<OsString> = Vec::new();
            if !overwrite {
                args.push("-n".into());
            }
            args.push(FILE_TRANSFER_PATH.into());

            // Handle various edge cases on `to`.
            match to.trim() {
                "." => {
                    // We need to use the source file name. Otherwise, we will wind up calling
                    // `mv FILE_TRANSFER_PATH .`, which is wrong.
                    let path = Path::new(&from)
                        .file_name()
                        .expect("Action::Upload::from should be a file, not a directory");
                    args.push(path.into())
                }
                to if Path::new(to).is_dir() => {
                    // `to` is a directory, so we need to add the source file name to the
                    // destination. Otherwise, we will implicitly use FILE_TRANSFER_PATH as the
                    // file name.
                    let file_name = Path::new(&from)
                        .file_name()
                        .expect("Action::Upload::from should be a file, not a directory");
                    let path = Path::new(to).join(file_name);
                    args.push(path.into())
                }
                // Intentionally unhandled case: "~" - almost certainly not what the user meant,
                // but the docs warned about this, so we'll trust the user.
                _ => args.push(to.into()),
            };

            if let Err(e) = client::run("mv", &args) {
                // Try to delete the temporary file for security, but if that fails, silently
                // ignore the failure. Either way, return the error from `mv`.
                //
                // We need to invoke `rm` instead of of using std::fs so we can resolve the path
                // the same way as `mv` and the other commands.
                let _ = client::run("rm", &[FILE_TRANSFER_PATH]);
                return Err(e);
            }
        }
    }
    Ok(())
}

// Run a command locally, and if it fails, return a descriptive Err value.
//
// `command_string` should be a precise text-form equivalent of `command`. If `command_string` is
// `None`, then an approximation will be used in the event of an error.
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
