//! Sign and verify files with SSH keys.

use crate::config;
use anyhow::{anyhow, bail, Context};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The subdirectory within Sira's configuration directory that holds SSH keys.
const KEY_DIR: &str = "keys";

/// The subdirectory within Sira's configuration directory that holds SSH allowed signers files.
const ALLOWED_SIGNERS_DIR: &str = "allowed_signers";

// TODO Consider replacing key and allowed_signers args to sign and verify with &'static str to
// guarantee no directory traversal attacks and enable compiler optimizations.

/// Returns the path to the directory for a resource type.
///
/// `name` should be one of the constants defined in this file, e.g. [KEY_DIR].
///
/// Returns the path to [config::config_dir] plus `name`.
fn resource_dir(name: &'static str) -> PathBuf {
    let mut dir = config::config_dir();
    dir.push(name);
    dir
}

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
        // unsafe behavior, but if we don't catch it here, we will return an unhelpful error.
        bail!("key should not be empty");
    }

    let key_dir = resource_dir(KEY_DIR);
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

/// Verifies the cryptographic signature of a buffer, as generated by [sign].
///
/// Verifies that `signature` is a valid cryptographic signature of `file`. Searches for a public
/// key identified by `identity` inside of `allowed_signers`.
///
/// `signature` may be any accessible path.
///
/// `allowed_signers` is the name of a file in Sira's allowed signers directory and may only
/// contain alphabetic characters. In practice, the caller should only pass values for
/// `allowed_signers` that correspond to those in Sira's documentation, but that is not checked
/// here.
///
/// `identity` may be any valid identity as specified by OpenSSH for use with
/// `ssh-keygen -Y verify`.
///
/// # Returns
///
/// If verification succeeds, returns `Ok(())`. Otherwise, returns an error message with
/// information about the failure.
///
/// # Caller's security considerations
///
/// As part of Sira's security scheme, if well-known locations are populated with public keys, then
/// certain values **must** be signed: unsigned values must be rejected. Due to the call signature
/// of `ssh-keygen -Y verify` and related implementation considerations, this function requires a
/// signature. Therefore, it is up to the caller to reject unsigned values when the relevant public
/// key is present.
//
// TODO Add a public helper function in this module to check whether a key is present.
pub fn verify(
    file: &[u8],
    signature: impl AsRef<OsStr>,
    allowed_signers: impl AsRef<Path>,
    identity: impl AsRef<str>,
) -> anyhow::Result<()> {
    // Guard against directory traversal attacks. We don't plan to accept user-supplied values,
    // so this is just a hyper-restrictive cursory check for extra safety.
    if allowed_signers.as_ref().to_str().is_none()
        || allowed_signers
            .as_ref()
            .to_str()
            .unwrap()
            .chars()
            .any(|c| !c.is_alphabetic())
    {
        bail!(
            "allowed_signers should only contain alphabetic characters: {:?}",
            allowed_signers.as_ref(),
        );
    } else if allowed_signers.as_ref().to_str().unwrap().trim().is_empty() {
        // An empty allowed_signers arguably a variant on a directory traversal attack. It won't
        // cause unsafe behavior, but if we don't catch it here, we will return an unhelpful error.
        bail!("allowed_signers should not be empty");
    }

    let allowed_signers_dir = resource_dir(ALLOWED_SIGNERS_DIR);
    let allowed_signers_file = allowed_signers_dir.join(allowed_signers);

    // There is a TOCTOU issue with checking for allowed_signers_file's existence here and then calling
    // ssh-keygen. Since ssh-keygen and this function both operate safely regardless of this check,
    // there shouldn't be a security concern. The only critical issue is that if ssh-keygen returns
    // an error, then we must return that error.
    //
    // The goal with this check is simply to offer a more helpful error message than the default
    // from OpenSSH.
    if let Ok(false) = allowed_signers_file.try_exists() {
        bail!(
            "please install the required allowed_signers file: {}",
            allowed_signers_file.to_string_lossy(),
        );
    }

    // ssh-keygen \
    //   -Y verify \
    //   -f <allowed-signers-file> \
    //   -I <identity> \
    //   -n sira \
    //   -s <signature-file> \
    //   < <file-to-verify>
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(allowed_signers_file)
        .arg("-I")
        .arg(identity.as_ref())
        .args(["-n", "sira"])
        .arg("-s")
        .arg(signature)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn ssh-keygen child process")?;

    // Give ssh-keygen the file to verify.
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
        true => Ok(()),
        false => Err(anyhow!("{}", String::from_utf8_lossy(&output.stderr))),
    }
}

#[cfg(test)]
mod test;
