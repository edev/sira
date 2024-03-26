use super::*;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::NamedTempFile;

// Returns paths to a file inside of the /resources directory of the repository.
// This function simply computes the path but does not check whether the path exists.
fn resource_path(file_path: impl AsRef<Path>) -> PathBuf {
    let mut path = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    path.push(file_path);
    path
}

// Signs a file in the file system (rather than in memory). Writes the resulting signature to the
// file system. Overwrites any existing signature file.
//
// In the event of a successful signing, the second return value will contain the path to a
// temporary file containing the signature. It is the caller's responsibility to delete this file.
fn sign_file(
    file: impl AsRef<Path>,
    key: impl AsRef<Path>,
) -> anyhow::Result<(SigningOutcome, Option<PathBuf>)> {
    let mut file_buffer = Vec::new();
    let _ = File::open(&file)?.read_to_end(&mut file_buffer)?;
    let signing_outcome = sign(&file_buffer, key)?;
    let mut signature_file_path = None;
    if let SigningOutcome::Signed(ref signature) = signing_outcome {
        let (mut file, path) = NamedTempFile::new()?.keep()?;
        file.write_all(signature)?;
        signature_file_path = Some(path);
    }
    Ok((signing_outcome, signature_file_path))
}

// Reads a file from disk and calls `verify`.
fn verify_file(
    file: impl AsRef<Path>,
    signature: impl AsRef<OsStr>,
    allowed_signers: impl AsRef<Path>,
    identity: impl AsRef<str>,
) -> anyhow::Result<()> {
    let mut file_buffer = Vec::new();
    let _ = File::open(&file)?.read_to_end(&mut file_buffer)?;
    verify(&file_buffer, signature, allowed_signers, identity)
}

mod sign {
    use super::*;

    // I am not aware of any feasible way to test the various errors that can crop up when dealing
    // with std::process::Command. Thus, these are not under test here.

    #[test]
    fn if_signing_succeeds_returns_signature() {
        let file_path = resource_path("sample.manifest");

        let key = "manifest";

        // Run the code under test using the sign_file wrapper.
        let signature_path = match sign_file(&file_path, key) {
            Ok((SigningOutcome::Signed(_), Some(signature_path))) => signature_path,
            x => panic!("expected Ok((SigningOutcome::Signed(_), Some(path)) but received:\n{x:?}"),
        };

        // Now we verify that the signature is actually correct. We intentionally use a manual
        // implementation here with as much static information as possible rather than depending on
        // crate::crypto::verify to prevent the possibility of codependent bugs in sign() and
        // verify() causing this test to erroneously pass.

        let allowed_signers_path = resource_dir(ALLOWED_SIGNERS_DIR).join(key);

        let signer_identity = {
            let mut allowed_signers = fs::read_to_string(&allowed_signers_path).unwrap();
            let identity_length = allowed_signers.find(' ').unwrap();
            allowed_signers.truncate(identity_length);
            allowed_signers
        };

        // ssh-keygen \
        //     -Y verify \
        //     -f <allowed-signers> \
        //     -I <signer-identity> \
        //     -n sira \
        //     -s <signature-file> \
        //     < <file-to-verify>
        let output = Command::new("ssh-keygen")
            .args(["-Y", "verify", "-f"])
            .arg(&allowed_signers_path)
            .arg("-I")
            .arg(&signer_identity)
            .args(["-n", "sira", "-s"])
            .arg(&signature_path)
            .stdin(File::open(file_path).unwrap())
            .output()
            .unwrap();

        // Remove the signature file before asserting success.
        fs::remove_file(&signature_path).unwrap();

        assert!(output.status.success(), "{output:?}");
    }

    #[test]
    fn protects_from_directory_traversal_on_key() {
        let file = resource_path("sample.manifest");
        let file = {
            let mut file_buffer = Vec::new();
            let _ = File::open(file)
                .unwrap()
                .read_to_end(&mut file_buffer)
                .unwrap();
            file_buffer
        };

        // These are not all possible types of directory traversals, but checking these will
        // exercise the logic that requires keys to be alphabetic. This, in turn, prevents
        // directory traversal.
        for key in ["/", "..", "/home"] {
            match sign(&file, key) {
                Err(anyhow) if anyhow.to_string().contains("alphabetic characters") => {}
                x => panic!(
                    "failed to detect directory traversal attack on key \"{key}\"; instead \
                    returned: {x:?}"
                ),
            }
        }
    }

    #[test]
    fn rejects_empty_key_argument() {
        let file = resource_path("sample.manifest");
        let file = {
            let mut file_buffer = Vec::new();
            let _ = File::open(file)
                .unwrap()
                .read_to_end(&mut file_buffer)
                .unwrap();
            file_buffer
        };
        match sign(&file, "") {
            Err(anyhow) if anyhow.to_string().contains("should not be empty") => {}
            x => panic!("failed to detect empty key; instead returned: {x:?}"),
        }
    }

    #[test]
    fn if_key_does_not_exist_returns_key_not_found() {
        let file = resource_path("sample.manifest");
        let file = {
            let mut file_buffer = Vec::new();
            let _ = File::open(file)
                .unwrap()
                .read_to_end(&mut file_buffer)
                .unwrap();
            file_buffer
        };

        match sign(&file, "doesnotexist") {
            Ok(SigningOutcome::KeyNotFound) => {}
            x => panic!("expected Ok(SigningOutcome::KeyNotFound), but received: {x:?}"),
        }
    }

    #[test]
    #[should_panic(expected = "keys/unreadable")]
    fn returns_error_from_command() {
        let file = [];

        let unreadable_path = resource_dir(KEY_DIR).join("unreadable");

        // Create an unreadable private key file. If it already exists (e.g. from a previously
        // terminated test pass), this will fail, and that's fine. After this line, though, all
        // calls must succeed in order for the test to pass and are therefore unwrapped.
        let _ = File::create(&unreadable_path);

        // Remove all permissions on the key file.
        let mut permissions = fs::metadata(&unreadable_path).unwrap().permissions();
        permissions.set_mode(0o0);
        fs::set_permissions(&unreadable_path, permissions).unwrap();

        // Hand ssh-keygen a key file it can't read, so it returns with an error message.
        //
        // Note: the expected error message is non-obvious. If ssh-keygen can't access the private
        // key, I believe it looks for the associated public key to try looking up the private key
        // in ssh-agent.
        let result = sign(&file, "unreadable");

        // Clean up the unreadable key file before we unwrap the result.
        fs::remove_file(unreadable_path).unwrap();

        result.unwrap();
    }
}

mod verify {
    use super::*;

    // I am not aware of any feasible way to test the various errors that can crop up when dealing
    // with std::process::Command. Thus, these are not under test here.

    #[test]
    fn if_verification_succeeds_returns_ok() {
        let file_path = resource_path("sample.manifest");

        // The name of both the private key used to sign and the allowed_signers file used to
        // verify the signature.
        let key = "manifest";

        // Sign the file using the sign_file wrapper.
        //
        // Note that there are no concerns about codependent bugs, here, because we have a test for
        // sign() that mitigates that concern. Therefore, we can just use sign_file() here.
        let signature_path = match sign_file(&file_path, key) {
            Ok((SigningOutcome::Signed(_), Some(signature_path))) => signature_path,
            x => panic!("expected Ok((SigningOutcome::Signed(_), Some(path)) but received:\n{x:?}"),
        };

        let allowed_signers_path = resource_dir(ALLOWED_SIGNERS_DIR).join(key);

        // This will fail if the sample allowed_signers file has more than one entry and the entry
        // that matches the signature is not the first one, but this is all we need for testing.
        let identity = {
            let mut allowed_signers = fs::read_to_string(allowed_signers_path).unwrap();
            let identity_length = allowed_signers.find(' ').unwrap();
            allowed_signers.truncate(identity_length);
            allowed_signers
        };

        let result = verify_file(file_path, &signature_path, key, identity);

        // Remove the signature file before asserting success.
        fs::remove_file(&signature_path).unwrap();

        result.unwrap();
    }

    #[test]
    fn protects_from_directory_traversal_on_allowed_signers() {
        let file = resource_path("sample.manifest");
        let file = {
            let mut file_buffer = Vec::new();
            let _ = File::open(file)
                .unwrap()
                .read_to_end(&mut file_buffer)
                .unwrap();
            file_buffer
        };

        // These are not all possible types of directory traversals, but checking these will
        // exercise the logic that requires keys to be alphabetic. This, in turn, prevents
        // directory traversal.
        for key in ["/", "..", "/home"] {
            match verify(&file, "/dev/null", key, "sira") {
                Err(anyhow) if anyhow.to_string().contains("alphabetic characters") => {}
                x => panic!(
                    "failed to detect directory traversal attack on allowed_signers \"{key}\"; \
                    instead returned: {x:?}"
                ),
            }
        }
    }

    #[test]
    fn rejects_empty_allowed_signers_argument() {
        let file = resource_path("sample.manifest");
        let file = {
            let mut file_buffer = Vec::new();
            let _ = File::open(file)
                .unwrap()
                .read_to_end(&mut file_buffer)
                .unwrap();
            file_buffer
        };
        match verify(&file, "/dev/null", "", "sira") {
            Err(anyhow) if anyhow.to_string().contains("should not be empty") => {}
            x => panic!("failed to detect empty allowed_signers; instead returned: {x:?}"),
        }
    }

    #[test]
    #[should_panic(expected = "install the required allowed_signers file")]
    fn allowed_signers_file_not_found() {
        let file = resource_path("sample.manifest");
        let file = {
            let mut file_buffer = Vec::new();
            let _ = File::open(file)
                .unwrap()
                .read_to_end(&mut file_buffer)
                .unwrap();
            file_buffer
        };

        verify(&file, "/dev/null", "doesnotexist", "sira").unwrap();
    }

    #[test]
    #[should_panic(expected = "allowed_signers/unreadable")]
    fn returns_error_from_command() {
        let file = resource_path("sample.manifest");
        let file_buffer = fs::read_to_string(&file).unwrap().into_bytes();

        // The name of the key to use for the portions of this test setup that require a
        // functioning key.
        let stand_in_key = "manifest";

        // Create a real signature so OpenSSH can reach the point of failing to read
        // allowed_signers/unreadable.
        let signature_path = match sign_file(&file, stand_in_key) {
            Ok((SigningOutcome::Signed(_), Some(signature_path))) => signature_path,
            x => panic!("expected Ok((SigningOutcome::Signed(_), Some(path)) but received:\n{x:?}"),
        };

        let unreadable_path = resource_dir(ALLOWED_SIGNERS_DIR).join("unreadable");

        // Create an unreadable allowed_signers file. If it already exists (e.g. from a previously
        // terminated test pass), this will fail, and that's fine. After this line, though, all
        // calls must succeed in order for the test to pass and are therefore unwrapped.
        let _ = File::create(&unreadable_path);

        // Remove all permissions on the allowed_signers file.
        let mut permissions = fs::metadata(&unreadable_path).unwrap().permissions();
        permissions.set_mode(0o0);
        fs::set_permissions(&unreadable_path, permissions).unwrap();

        // Hand ssh-keygen an allowed_signers file it can't read, so it returns an error message.
        let result = verify(&file_buffer, &signature_path, "unreadable", "sira");

        // Clean up the files we created before we unwrap the result from above. Delay unwrapping
        // the results of these clean-up operations until they're all complete.
        let delayed_results: Vec<std::io::Result<()>> = [unreadable_path, signature_path]
            .iter()
            .map(fs::remove_file)
            .collect();
        for result in delayed_results {
            result.unwrap();
        }

        result.unwrap();
    }
}
