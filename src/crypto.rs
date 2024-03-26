//! Sign and verify files with SSH keys.

use crate::config;
use anyhow::{anyhow, bail, Context};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// The subdirectory within Sira's configuration directory that holds SSH keys.
const KEY_DIR: &str = "keys";

/// The subdirectory within Sira's configuration directory that holds SSH allowed signers files.
#[allow(dead_code)]
const ALLOWED_SIGNERS_DIR: &str = "allowed_signers";

/// Success return values for the [sign] function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigningOutcome {
    /// Signing succeeded. The contained value is the signature.
    Signed(Vec<u8>),

    /// The key was not installed. This is fine.
    KeyNotFound,
}

/// Cryptographically signs the contents of a buffer with an SSH key, if the key exists.
///
/// Signs `file` with the key named `key` in Sira's key directory. `key` may only contain
/// alphabetic characters. In practice, the caller should only pass values for `key` that
/// correspond to those in Sira's documentation, but that is not checked here.
///
/// In order to preserve the system administrator's freedom to configure their systems as they see
/// fit, the security of key files and the key directory are not checked here.
///
/// # Returns
///
/// If signing succeeds, returns an [Ok] value containing the signature.
///
/// If `key` was not present in the key directory, returns an [Ok] value containing
/// [SigningOutcome::KeyNotFound].
///
/// If `key` was not alphabetic, `key` was empty, or the attempt to sign with `ssh-keygen` failed
/// for any reason, returns an [Err] value.
pub fn sign(file: &[u8], key: impl AsRef<Path>) -> anyhow::Result<SigningOutcome> {
    // Guard against directory traversal attacks. We don't plan to accept user-supplied values,
    // so this is just a hyper-restrictive cursory check for extra safety.
    if key.as_ref().to_str().is_none()
        || key
            .as_ref()
            .to_str()
            .unwrap()
            .chars()
            .any(|c| !c.is_alphabetic())
    {
        bail!(
            "key should only contain alphabetic characters: {:?}",
            key.as_ref(),
        );
    } else if key.as_ref().to_str().unwrap().trim().is_empty() {
        // An empty key arguably a variant on a directory traversal attack. It won't result in
        // unsafe behavior, but if we don't catch it here, it will result in unhelpful or harmful
        // error messages, presumably a permission error on the directory telling the user,
        // incorrectly, to chmod the directory to 0400.
        bail!("key should not be empty");
    }

    let key_dir = {
        let mut dir = config::config_dir();
        dir.push(KEY_DIR);
        dir
    };
    let key_file = key_dir.join(key);

    // There is a TOCTOU issue with checking for key_file's existence here and then calling
    // ssh-keygen. Since ssh-keygen and this function both operate safely regardless of this check,
    // there shouldn't be a security concern. The only critical issue is that if ssh-keygen returns
    // an error, then we must return that error.
    if let Ok(false) = key_file.try_exists() {
        return Ok(SigningOutcome::KeyNotFound);
    }

    // ssh-keygen -Y sign -f <key-file> -n sira
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "sign", "-f"])
        .arg(key_file)
        .args(["-n", "sira"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn ssh-keygen child process")?;

    // Give ssh-keygen the file to sign.
    let mut child_stdin = child
        .stdin
        .take()
        .expect("failed to retrieve ssh-keygen's stdin");
    child_stdin
        .write_all(file)
        .context("failed to write to ssh-keygen's stdin")?;
    child_stdin
        .flush()
        .context("failed to flush ssh-keygen's stdin")?;
    drop(child_stdin);

    let output = child
        .wait_with_output()
        .context("failed to wait_with_output for ssh-keygen")?;
    match output.status.success() {
        true => Ok(SigningOutcome::Signed(output.stdout)),
        false => Err(anyhow!("{}", String::from_utf8_lossy(&output.stderr))),
    }
}

#[cfg(test)]
mod test;
