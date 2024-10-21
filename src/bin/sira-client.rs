use anyhow::{anyhow, bail, Context};
use shlex::Shlex;
use sira::client;
use sira::core::action::{line_in_file, script, Action, FILE_TRANSFER_PATH};
use sira::crypto;
use std::env;
use std::ffi::OsString;
use std::fs;
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

    // Note that we read in the YAML as a String rather than parsing it as YAML. We don't want to
    // expose the parser to untrusted YAML, so we parse the YAML only after signature verification.
    //
    // Also note that for security, crypto::verify reads the string to verify from a buffer rather
    // than a file. This avoids the security-critical TOCTOU issue of verifying a file on disk and
    // then reading it in.
    let action_file = env::args().nth(1).expect(
        "missing required argument <action-as-yaml>, but failed to detect this and display a \
        helpful error message to the user",
    );
    let yaml = fs::read_to_string(&action_file)
        .with_context(|| format!("sira-client could not read action file: {action_file}"))?;

    if require_signature {
        let signature_file = env::args().nth(2).expect(
            "missing required argument <action-signature>, but failed to detect this and display a \
            helpful error message to the user",
        );

        crypto::verify(
            yaml.as_bytes(),
            &signature_file,
            ALLOWED_SIGNERS_FILE,
            "sira",
        )?;

        // Make an effort to clean up the file if it's obviously a temporary file.
        if signature_file.starts_with("/tmp") {
            fs::remove_file(&signature_file).with_context(|| {
                format!("sira-client could not remove temporary signature file: {signature_file}")
            })?;
        }
    }

    fs::remove_file(&action_file).with_context(|| {
        format!("sira-client could not remove temporary action file: {action_file}")
    })?;

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

            // Populate our args Vec with the source path.
            let mut args: Vec<OsString> = vec![FILE_TRANSFER_PATH.into()];

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

            // If we're not to overwrite the destination, do a last-minute check that it doesn't
            // exist. There is an inevitable TOCTOU concern here, but checking right before we call
            // `mv` is the best we're going to do.
            if !overwrite && fs::exists(&args[1])? {
                return Ok(fs::remove_file(FILE_TRANSFER_PATH)?);
            }
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

    sira-client <action-file> [<action-signature-file>]

The first argument is the path to a YAML file containing a single Action.

The second argument is the path to a cryptographic signature for this action, generated by \
invoking `ssh-keygen -Y sign` on the action. This is required if the allowed signers file is \
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
