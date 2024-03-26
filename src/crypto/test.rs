use super::*;
use std::fmt::Display;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

// Returns the path to the /resources directory of the repository.
fn resource_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

// Returns paths to a file and its signature inside of the /resources directory of the repository.
// This function does not check whether either path exists; it simply computes the paths.
fn file_signature_pair(file_path: impl Display + AsRef<Path>) -> (PathBuf, PathBuf) {
    let base_path = resource_path();
    let signature_path = format!("{file_path}.sig");
    (base_path.join(file_path), base_path.join(signature_path))
}

// Signs a file in the file system (rather than in memory). Writes the resulting signature to the
// file system. Overwrites any existing signature file.
fn sign_file(file: impl AsRef<Path>, key: impl AsRef<Path>) -> anyhow::Result<SigningOutcome> {
    let mut file_buffer = Vec::new();
    let _ = File::open(&file)?.read_to_end(&mut file_buffer)?;
    let signing_outcome = sign(&file_buffer, key)?;
    if let SigningOutcome::Signed(ref signature) = signing_outcome {
        let mut signature_file_name = file
            .as_ref()
            .file_name()
            .expect("signed a file on disk but its path didn't end in a file name")
            .to_owned();
        signature_file_name.push(".sig");

        let signature_path = file
            .as_ref()
            .parent()
            .expect("signed a file on disk but couldn't retrieve its parent directory")
            .join(signature_file_name);

        fs::write(signature_path, signature)?;
    }
    Ok(signing_outcome)
}

mod sign {
    use super::*;

    // I am not aware of any feasible way to test the various errors that can crop up when dealing
    // with std::process::Command. Thus, these are not under test here.

    #[test]
    fn if_signing_succeeds_returns_signature() {
        let (file_path, signature_path) = file_signature_pair("sample.manifest");

        let key = "manifest";

        // Remove any previous signature, e.g. in case of a crashed test.
        //
        // We don't need to parse specific errors here, because if we can't remove the file, we
        // will presumably encounter errors trying to write the file in sign_file, which handles
        // and reports errors.
        let _ = fs::remove_file(&signature_path);

        // Run the code under test using the sign_file wrapper.
        match sign_file(&file_path, key) {
            Ok(SigningOutcome::Signed(_)) => {}
            x => panic!("expected Ok(SigningOutcome::Signed(_)) but received:\n{x:?}"),
        };

        // Now we verify that the signature is actually correct. We intentionally use a manual
        // implementation here with as much static information as possible rather than depending on
        // crate::crypto::verify to prevent the possibility of codependent bugs in sign() and
        // verify() causing this test to erroneously pass.

        let allowed_signers_path = {
            let mut path = config::config_dir();
            path.push(ALLOWED_SIGNERS_DIR);
            path.push(key);
            path
        };

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
        let (file, _) = file_signature_pair("sample.manifest");
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
                Err(anyhow) if anyhow.to_string().contains("alphabetic characters") => {},
                x => panic!("failed to detect directory traversal attack on key \"{key}\"; instead returned: {x:?}"),
            }
        }
    }

    #[test]
    fn rejects_empty_key_argument() {
        let (file, _) = file_signature_pair("sample.manifest");
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
        let (file, _) = file_signature_pair("sample.manifest");
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

        let unreadable_path = {
            let mut path = config::config_dir();
            path.push(KEY_DIR);
            path.push("unreadable");
            path
        };

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
