//! An installer for sira-client.
//!
//! For usage details, please see [installation.md].
//!
//! [installation.md]: https://github.com/edev/sira/blob/main/installation.md

// TODO Try to statically link everything. Examine what's dynamically linked and how it's licensed.

use sira::client;
use sira::config;
use sira::core::action::{self, Action};
use sira::crypto::{ALLOWED_SIGNERS_DIR, KEY_DIR};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

// SECTION: installer & client binaries

/// The file name of the installer binary (this file).
const INSTALLER_BIN: &str = "sira-install";

/// The file name of the client binary.
const CLIENT_BIN: &str = "sira-client";

/// The installation directory for the client binary on managed nodes.
const CLIENT_INSTALL_DIR: &str = "/opt/sira/bin";

// SECTION: SSH keys

/// The user's SSH directory on both control and managed nodes, relative to the user's home.
const SSH_DIR: &str = ".ssh";

/// The SSH private key used to log in via SSH as the Sira user on managed nodes.
const LOGIN_KEY: &str = "sira";

/// The SSH private key used to sign manifest and task files, known as the "manifest key".
const MANIFEST_KEY: &str = "manifest";

/// The SSH private key used to sign actions sent to the client binary, known as the "action key".
const ACTION_KEY: &str = "action";

/// Indicates whether a public key is present in one of several locations.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PublicKeyState {
    /// The public key is already installed as an allowed signers file.
    AllowedSignersFile,

    /// The public key is available in some location that the caller knows and expects.
    ///
    /// Most likely, the key is not installed as an allowed signers file, but these semantics are
    /// up to the code that creates the value.
    PublicKeyFile,

    /// Cannot find the public key.
    NotPresent,
}

// SECTION: SSH key flags

/// The name of the flag file for the manifest key.
///
/// If the key files don't exist, but this flag does, this indicates that we have already prompted
/// the administrator to generate this key during a previous program run and they have declined.
/// The flag file allows us to remember this choice across program invocations.
///
/// Thus, if the key is missing but the flag file is present, we will simply skip the key.
const MANIFEST_FLAG: &str = ".sira-install-skip-manifest-key";

/// The name of the flag file for the action key.
///
/// If the key files don't exist, but this flag does, this indicates that we have already prompted
/// the administrator to generate this key during a previous program run and they have declined.
/// The flag file allows us to remember this choice across program invocations.
///
/// Thus, if the key is missing but the flag file is present, we will simply skip the key.
const ACTION_FLAG: &str = ".sira-install-skip-action-key";

/// Program entry point.
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 3 {
        if args[1] == "--managed-node" {
            // sira-install --managed-node <sira-user>
            //
            // Managed node invocation is intentionally undocumented in the public interface. It is
            // meant for internal use only.
            managed_node(&args[2]);
        } else {
            // sira-install <sira-user> [<admin-user>@]<managed-node>
            control_node(&args[1], &args[2]);
        }
    } else {
        println!("Please see the installation guide for instructions.");
    }
}

// TODO Strongly consider moving this to crypto.rs and deploying globally. Same with key_dir().
/// Returns the path to the allowed signers directory.
fn allowed_signers_dir() -> &'static Path {
    static COMPUTED: OnceLock<PathBuf> = OnceLock::new();

    COMPUTED
        .get_or_init(|| {
            let mut dir = config::config_dir();
            dir.push(ALLOWED_SIGNERS_DIR);
            dir
        })
        .as_ref()
}

/// Installer logic that should run on the control node as the control node user.
fn control_node(sira_user: &str, destination: &str) {
    // Compute the Cargo bin directory, typically ~/.cargo/bin
    let cargo_bin_dir = {
        let mut cargo_home = home::cargo_home().expect("could not retrieve Cargo directory");
        cargo_home.push("bin");
        cargo_home
    };

    // Compute the user's home directory.
    let home_dir = home::home_dir().expect("could not retrieve user's home directory");

    // List of files to transfer to managed node (in a moment); this is dynamic, so we'll add to it
    // as we proceed.
    let mut file_transfers = vec![
        cargo_bin_dir.join(INSTALLER_BIN),
        cargo_bin_dir.join(CLIENT_BIN),
    ];

    // Compute the current user's SSH key directory, i.e. ~/.ssh
    let ssh_dir = home_dir.join(SSH_DIR);

    // Ensure that the login SSH key pair exists.
    //
    // The control node needs to use the private key. The public key is not security sensitive, and
    // we need to deploy it to managed nodes or else Sira won't work. Therefore, we expect to find
    // either both keys or neither key (in which case we generate a key pair). If we find one key
    // but not the other, that's an error, and there is no sensible way to proceed.
    {
        let private_exists = key_exists(&ssh_dir, LOGIN_KEY);
        let public_exists = key_exists(&ssh_dir, public_key(LOGIN_KEY));
        if private_exists ^ public_exists {
            panic!(
                // Wrapped to 80 characters.
                "found one half of a required key pair in ~/.ssh:\n\
                {} (present: {})\n\
                {} (present: {})\n\
                \n\
                Please either install the missing file or remove the existing file.",
                LOGIN_KEY,
                private_exists,
                public_key(LOGIN_KEY).display(),
                public_exists,
            );
        } else if !private_exists && !public_exists {
            let key_path = ssh_dir.join(LOGIN_KEY);
            println!(
                // Wrapped to 80 characters.
                "Generating SSH login key: {}\n\
                \n\
                The public key will be deployed to managed nodes to authenticate the Sira user.\n\
                The private key should remain on the control node. Protecting this key with a\n\
                password is highly recommended.\n",
                key_path.display(),
            );
            ssh_keygen(key_path);
        }
        file_transfers.push(ssh_dir.join(public_key(LOGIN_KEY)));
    }

    create_config_dirs();

    // Check for the manifest public key, and prompt to generate a key pair if it's absent.
    //
    // The manifest public key may be present as either a public key file in the user's SSH
    // directory or an allowed signers file installed on the control node (the current system).
    //
    // If this is a working network rather than a newly initialized one, then the manifest private
    // key is most likely absent for security reasons. This is proper and expected. On the other
    // hand, if this is a new network being spun up, then the manifest private key might be
    // present, and that's fine, too.
    //
    // However, if the private key is present but the public key is missing, that's an error, and
    // there is no sensible way to proceed.
    //
    // If the public key is present as a key file and there is no allowed signers file, then write
    // the allowed signers file.
    {
        let private_exists = key_exists(&ssh_dir, MANIFEST_KEY);
        let mut public_state = check_public_key(&ssh_dir, MANIFEST_KEY);
        if public_state == PublicKeyState::NotPresent {
            if private_exists {
                panic!(
                    // Wrapped to 80 characters.
                    "found the manifest private key in {key_dir}, but could not find the public\n\
                    key or the corresponding allowed signers file. Please do one of the \
                    following:\n\
                    \n\
                    1. Install the public key to {key_dir}\n\
                    2. Install the corresponding allowed signers file\n\
                    3. Remove the private key if you no longer wish to use it",
                    key_dir = ssh_dir.display(),
                );
            }
            let key_created =
                prompt_to_generate_signing_key_pair(&ssh_dir, MANIFEST_KEY, MANIFEST_FLAG);
            if key_created {
                public_state = PublicKeyState::PublicKeyFile;
            }
        }

        if public_state == PublicKeyState::PublicKeyFile {
            install_allowed_signers_file(&ssh_dir, MANIFEST_KEY);
        }
    }

    // Install action key (both public and private portions), if present. Prompt to generate a new
    // key pair if no existing key pair is found.
    //
    // We need the private key in order to sign actions on the control node (the current system).
    // OpenSSH requires the public key to be present in the same directory when asked to sign
    // files, so we need it as well. Plus, we need to deploy the public key to managed nodes in the
    // code below. Therefore, if we find one key but not the other, that's an error, and there is
    // no sensible way to proceed.
    //
    // The logic above applies to key pairs in Sira's key directory and key pairs in ~/.ssh. For
    // sanity and simplicity, the code below DOES NOT allow mixing and matching between these two
    // directories.
    {
        // Check for keys in Sira's key dir.
        let private_installed = key_exists(key_dir(), ACTION_KEY);
        let public_installed = key_exists(key_dir(), public_key(ACTION_KEY));
        if private_installed ^ public_installed {
            panic!(
                // Wrapped to 80 characters.
                "found one half of a key pair in {}:\n\
                {} (present: {})\n\
                {} (present: {})\n\
                \n\
                Please either install the missing file or remove the existing file.",
                key_dir().display(),
                ACTION_KEY,
                private_installed,
                public_key(ACTION_KEY).display(),
                public_installed,
            );
        }

        // Check for keys in ~/.ssh.
        let private_exists = key_exists(&ssh_dir, ACTION_KEY);
        let public_exists = key_exists(&ssh_dir, public_key(ACTION_KEY));
        if private_exists ^ public_exists {
            panic!(
                // Wrapped to 80 characters.
                "found one half of a key pair in ~/.ssh:\n\
                {} (present: {})\n\
                {} (present: {})\n\
                \n\
                Please either install the missing file or remove the existing file.",
                ACTION_KEY,
                private_exists,
                public_key(ACTION_KEY).display(),
                public_exists,
            );
        }

        // Generate and install the key, if needed.
        //
        // Note: the logic below depends on the XOR checks above.
        let transfer_key = if private_installed {
            true
        } else if private_exists
            || prompt_to_generate_signing_key_pair(&ssh_dir, ACTION_KEY, ACTION_FLAG)
        {
            install_signing_key_pair(&ssh_dir, ACTION_KEY);
            true
        } else {
            false
        };

        if transfer_key {
            file_transfers.push(key_dir().join(public_key(ACTION_KEY)));
        }
    }

    // Transfer files to managed node via SCP:
    //  - sira-install (required)
    //  - sira-client  (required)
    //  - sira.pub     (required)
    //  - action.pub   (optional)
    //
    // Invocation;
    // scp <file_transfers> <destination>:
    // TODO Add white space after declining to generate action key.
    {
        println!("Transferring files to {destination}");
        let mut args: Vec<&OsStr> = file_transfers
            .iter()
            .map(|s| -> &OsStr { s.as_ref() })
            .collect();
        let destination = format!("{destination}:");
        args.push(destination.as_ref());
        client::run("scp", &args).expect("error transferring files");
        println!();
    }

    // SSH over to the managed node using the destination from the command-line arguments. Run:
    //
    // ssh -t <destination> sudo ./sira-install --managed-node <sira-user>
    //
    // Be sure to use std::process::Command::new("ssh") rather than the openssh crate, because we
    // specifically WANT stdio to be piped to enable password-protected sudo in this case. The `-t`
    // argument makes it interactive, so sudo can prompt for a password.
    {
        println!("Running {INSTALLER_BIN} via sudo on {destination}");
        let command = format!("./{INSTALLER_BIN}");
        let args = [
            "-t",
            &destination,
            "sudo",
            &command,
            "--managed-node",
            &sira_user,
        ];
        client::run("ssh", &args).expect("error running installer on managed node");
    }
}

/// Checks whether a public key is present either in the public key directory or as an allowed
/// signers file.
///
/// If the public key is in both locations, the allowed signers file takes precedence.
///
/// Panics if unable to check either location.
fn check_public_key(
    public_key_directory: impl AsRef<Path>,
    name: impl AsRef<str>,
) -> PublicKeyState {
    if key_exists(allowed_signers_dir(), name.as_ref()) {
        return PublicKeyState::AllowedSignersFile;
    }

    let key_file = public_key(name.as_ref());
    if key_exists(public_key_directory, key_file) {
        return PublicKeyState::PublicKeyFile;
    }

    PublicKeyState::NotPresent
}

/// Creates a global configuration directory, if it doesn't exist.
fn create_config_dir(dir: impl AsRef<Path>) {
    if path_exists(dir.as_ref(), "config directory") {
        return;
    }

    println!(
        "Creating Sira config directory: {}\n\
        You might be prompted for your password one or more times.",
        dir.as_ref().display(),
    );
    client::run("sudo", &[OsStr::new("mkdir"), dir.as_ref().as_ref()])
        .expect("could not create config directory");

    let owner = "root:root";
    println!("Setting owner to {owner}");
    client::run(
        "sudo",
        &[
            OsStr::new("chown"),
            OsStr::new(owner),
            dir.as_ref().as_ref(),
        ],
    )
    .expect("could not chown config directory");

    let mode = "0755";
    println!("Setting mode to {mode}");
    client::run(
        "sudo",
        &[
            OsStr::new("chmod"),
            OsStr::new(&mode),
            dir.as_ref().as_ref(),
        ],
    )
    .expect("could not chmod config directory");
    println!();
}

/// Create the Sira config directory structure, if it doesn't exist.
fn create_config_dirs() {
    // We check for each directory separately because we are trying to be minimally invasive over
    // the administrator's owner, group, and permissions settings. For instance, if the config dir
    // exists but the key dir doesn't, we don't want to touch the config dir's owner, group, or mode.
    create_config_dir(config::config_dir());
    create_config_dir(allowed_signers_dir());
    create_config_dir(key_dir());
}

/// Reads a public key and installs it as an allowed signers file.
///
/// The key should be specified as a private key name, i.e. without the ".pub" extension.
fn install_allowed_signers_file(dir: impl AsRef<Path>, key_name: impl AsRef<Path>) {
    let key_path = {
        let file_name = public_key(&key_name);
        dir.as_ref().join(file_name)
    };

    let key = fs::read_to_string(&key_path).expect("could not read public key");

    {
        // Verify that the public key has only one line; we simply do not support multiple keys per
        // file, and as far as I know, OpenSSH might not, either.
        let key_file_lines = key.lines().count();
        assert!(
            key_file_lines == 1,
            "key file had {key_file_lines} lines but should only have 1:\n{}",
            key_path.display(),
        );
    }
    let mut components: Vec<&str> = key.trim().split(' ').collect();
    {
        // Verify that the key file has enough components to be plausible before we modify it and
        // write the result as root and then deploy the rest of Sira across a whole network.
        let num_components = components.len();
        assert!(
            // Format: [options] keytype base64-key comment
            num_components >= 3,
            // Wrapped to 80 characters.
            "key file had {num_components} components but should have at least 3; it seems\n\
            to be malformed: {}",
            key_path.display(),
        );
    }

    // Move the public key's comment to the start of the string to form the principal field in the
    // allowed signers file.
    let principal = components.pop().unwrap();
    components.insert(0, principal);
    let allowed_signers = {
        let mut line = components.join(" ");
        line.push('\n');
        line
    };

    // Sanity checks, since there are several tricky bits in reading, splitting, and reassembling
    // the public key file as an allowed signers file.
    debug_assert_eq!(key.len(), allowed_signers.len());
    debug_assert!(allowed_signers.starts_with("sira "));

    let allowed_signers_file = allowed_signers_dir().join(key_name);

    // Write the allowed signers file to a temp file.
    let (mut file, temp_file_path) = client::mktemp().expect("could not open temporary file");
    file.write_all(allowed_signers.as_bytes())
        .expect("error writing temporary file");
    file.flush().expect("error flushing temporary file");
    drop(file);

    println!(
        "Installing allowed signers file: {}\n\
        You might be prompted for your password one or more times.",
        allowed_signers_file.display(),
    );
    client::run(
        "sudo",
        &[
            OsStr::new("cp"),
            OsStr::new(&temp_file_path),
            allowed_signers_file.as_os_str(),
        ],
    )
    .expect("error copying allowed signers file to Sira config directory");

    let owner = "root:root";
    println!("Setting owner to {owner}");
    client::run(
        "sudo",
        &[
            OsStr::new("chown"),
            OsStr::new(owner),
            allowed_signers_file.as_ref(),
        ],
    )
    .expect("could not chown allowed signers file");

    // This probably isn't necessary, but for sanity, we'll set the new file's mode.
    let mode = "0644";
    println!("Setting mode to {mode}");
    client::run(
        "sudo",
        &[
            OsStr::new("chmod"),
            OsStr::new(&mode),
            allowed_signers_file.as_ref(),
        ],
    )
    .expect("could not chmod allowed signers file");
    println!("Allowed signers file installed.\n");
}

/// Copies the public and private files for an SSH key to Sira's key directory.
fn install_signing_key_pair(dir: impl AsRef<Path>, key_name: impl AsRef<Path>) {
    let private_key_file = dir.as_ref().join(key_name.as_ref());
    let public_key_file = dir.as_ref().join(public_key(&key_name));
    println!(
        "Installing {} key files to {}:\n\
        {}\n\
        {}\n\
        You might be prompted for your password one or more times.",
        key_name.as_ref().display(),
        key_dir().display(),
        private_key_file.display(),
        public_key_file.display(),
    );
    client::run(
        "sudo",
        &[
            OsStr::new("cp"),
            private_key_file.as_ref(),
            public_key_file.as_ref(),
            key_dir().as_ref(),
        ],
    )
    .expect("error copying key files to Sira config directory");

    let installed_private_key = key_dir().join(key_name.as_ref());
    let installed_public_key = key_dir().join(public_key(key_name.as_ref()));

    let owner = format!("root:{}", client::whoami());
    println!("Setting owner to {owner}");
    client::run(
        "sudo",
        &[
            OsStr::new("chown"),
            OsStr::new(&owner),
            installed_private_key.as_ref(),
        ],
    )
    .expect("could not chown private key");

    let owner = "root:root";
    println!("Setting owner to {owner}");
    client::run(
        "sudo",
        &[
            OsStr::new("chown"),
            OsStr::new(owner),
            installed_public_key.as_ref(),
        ],
    )
    .expect("could not chown public key");

    let mode = "0640";
    println!("Setting private key's mode to {mode}");
    client::run(
        "sudo",
        &[
            OsStr::new("chmod"),
            OsStr::new(&mode),
            installed_private_key.as_ref(),
        ],
    )
    .expect("could not chmod private key file");

    let mode = "0644";
    println!("Setting public key's mode to {mode}");
    client::run(
        "sudo",
        &[
            OsStr::new("chmod"),
            OsStr::new(&mode),
            installed_public_key.as_ref(),
        ],
    )
    .expect("could not chmod public key file");
    println!("Signing key installed.");
}

/// Returns the path to the directory where we store cryptographic signing keys.
fn key_dir() -> &'static Path {
    static COMPUTED: OnceLock<PathBuf> = OnceLock::new();

    COMPUTED
        .get_or_init(|| {
            let mut dir = config::config_dir();
            dir.push(KEY_DIR);
            dir
        })
        .as_ref()
}

/// Checks whether a key exists, panicking if we can't determine an answer.
///
/// This is a wrapper around [Path::try_exists]. It applies key-file-specific logic and error text.
fn key_exists(dir: impl AsRef<Path>, key_name: impl AsRef<Path>) -> bool {
    let path = dir.as_ref().join(key_name.as_ref());
    path_exists(path, "key file")
}

/// Installer logic that should run on the managed node as root.
fn managed_node(sira_user: &str) {
    // Move the login public key to the Sira user's ~/.ssh/authorized_keys, ensuring correct permissions.
    // Feel free to assume it's at /home/<sira-user>. If someone wants to deploy this in a funky
    // setup, they can write their own installer or modify this one; this is all well-documented.
    //
    // If you're reading this because you want to modify the installer, and you think your changes
    // will be useful to others as well, please feel free to open an issue to discuss them.
    {
        let sira_home_dir = Path::new("/home").join(sira_user);
        let sira_ssh_dir = sira_home_dir.join(".ssh");

        // Create the Sira user's ~/.ssh if it doesn't exist. If the Sira user doesn't exist,
        // that's outside our purview and will result in errors from the processes we call. We'll
        // try to detect it gracefully by checking for the Sira user's home directory, though.
        if !path_exists(&sira_ssh_dir, "the Sira user's SSH directory") {
            if !path_exists(&sira_home_dir, "the Sira user's home directory") {
                panic!(
                    "Could not find the Sira user's home directory: {}\n\
                    Have you created the Sira user on this machine?",
                    sira_home_dir.display(),
                );
            }

            println!("Creating the Sira user's ~/.ssh directory.");
            client::run("mkdir", &[&sira_ssh_dir]).expect("error creating directory");

            println!("Setting owner.");
            let owner = format!("{sira_user}:{sira_user}");
            client::run("chown", &[OsStr::new(&owner), sira_ssh_dir.as_ref()])
                .expect("error chowning directory");

            println!("Setting mode.");
            let mode = "0755";
            client::run("chmod", &[OsStr::new(mode), sira_ssh_dir.as_ref()])
                .expect("error chmodding directory");
            println!();
        }

        // For security, deliberately wipe out any existing contents of AUTHORIZED_KEYS.
        println!("Installing Sira user's public key as ~/.ssh/authorized_keys");
        let authorized_keys = sira_ssh_dir.join("authorized_keys");
        client::run("mv", &[&public_key(LOGIN_KEY), &authorized_keys])
            .expect("error moving key to ~/.ssh/authorized_keys");

        println!("Setting owner.");
        let owner = format!("{sira_user}:{sira_user}");
        client::run("chown", &[OsStr::new(&owner), authorized_keys.as_ref()])
            .expect("error chowning ~/.ssh/authorized_keys");

        println!("Setting mode.");
        let mode = "0644";
        client::run("chmod", &[OsStr::new(mode), authorized_keys.as_ref()])
            .expect("error chmodding ~/.ssh/authorized_keys");
        println!();
    }

    // Ensure existence of the client install directory. Don't mangle the administrator's owner,
    // group, or permissions: by default, this operation should require root, but if the
    // administrator is doing something different, we'll trust them to know what they're doing.
    if !path_exists(CLIENT_INSTALL_DIR, "client install directory") {
        println!("Creating {CLIENT_INSTALL_DIR}");
        client::run("mkdir", &["-p", CLIENT_INSTALL_DIR]).expect("error creating directory");
        println!();
    }

    // Move the client binary to the client install directory.
    //
    // Ensure correct user, group, & permissions.
    {
        println!("Installing {CLIENT_BIN} to {CLIENT_INSTALL_DIR}");
        client::run("mv", &[CLIENT_BIN, CLIENT_INSTALL_DIR]).expect("error moving client binary");

        println!("Setting owner.");
        let client_path = Path::new(CLIENT_INSTALL_DIR).join(CLIENT_BIN);
        let owner = "root:root";
        client::run("chown", &[OsStr::new(owner), client_path.as_ref()])
            .expect("error chowning client binary");

        println!("Setting mode.");
        let mode = "0700";
        client::run("chmod", &[OsStr::new(mode), client_path.as_ref()])
            .expect("error chmodding client binary");
        println!();
    }

    // Install the Sira user in sudoers, idempotently.
    //
    // If /etc/sudoers.d exists, use it. Otherwise, modify /etc/sudoers.
    {
        // Components of the eventual Action::LineInFile; we will shadow these if needed.
        let path = "/etc/sudoers";
        let line = format!("{sira_user}\tALL=(root:root) NOPASSWD:/opt/sira/bin/sira-client");
        let pattern = None;
        let after = None;
        let indent = false;

        println!("Installing {sira_user} as a sudoer.");
        if path_exists("/etc/sudoers.d", "/etc/sudoers.d directory") {
            let path = "/etc/sudoers.d/10_sira";

            if !path_exists(path, "Sira sudoers file") {
                println!("Creating file: {path}");
                let _ = File::create(path).expect("could not create sudoers file");

                println!("Setting mode.");
                client::run("chmod", &["0640", path]).expect("error chmodding sudoers file");
            }

            println!("Updating {path} with entry for {sira_user}");
            action::line_in_file(&Action::LineInFile {
                path: path.to_string(),
                line,
                pattern,
                after,
                indent,
            })
            .expect("error updating sudoers file");
        } else {
            println!("Updating {path} with entry for {sira_user}");
            action::line_in_file(&Action::LineInFile {
                path: path.to_string(),
                line,
                pattern,
                after,
                indent,
            })
            .expect("error updating sudoers file");
        }
        println!();
    }

    create_config_dirs();

    // If present in the CWD, install the action allowed signers file. Remember to move the
    // identity from the end to the start. If it already exists, do not replace it.
    {
        let cwd = ".";
        let key = ACTION_KEY;
        if key_exists(cwd, public_key(key)) {
            install_allowed_signers_file(cwd, key);

            // Remove action.pub.
            fs::remove_file(Path::new(cwd).join(public_key(key)))
                .expect("error cleaning up action public key");
        }
    }

    // Remove the installer; we're done with it.
    fs::remove_file(INSTALLER_BIN).expect("error cleaning up installer");
}

/// Wraps [Path::try_exists] with a panic in case of failure
///
/// `desc` is a very brief description of the file type.
fn path_exists(path: impl AsRef<Path>, desc: impl Display) -> bool {
    match path.as_ref().try_exists() {
        Ok(x) => x,
        Err(e) => panic!(
            "could not determine whether {} exists: {}\n{}",
            desc,
            path.as_ref().display(),
            e,
        ),
    }
}

/// Prompts the user for consent to generate an SSH key pair used for cryptographic signing.
///
/// The user interactions in this function explain the importance of the manifest and action keys
/// before prompting the user for consent to enable them.
///
/// If `flag_file` exists, returns `false` immediately.
///
/// Otherwise, asks the user if they would like to generate the given SSH key.
///
/// Returns whether the key pair was generated.
fn prompt_to_generate_signing_key_pair(
    dir: impl AsRef<Path>,
    key_name: impl AsRef<Path>,
    flag_file: impl AsRef<Path>,
) -> bool {
    // Implementation detail: the flag file is in the CWD, not starting_directory.
    //
    // This is entirely arbitrary.
    if path_exists(&flag_file, "flag file") {
        return false;
    }

    // Whether this is the first time during this program invocation that we have visited this
    // function. On first run, we will print help text.
    //
    // After the first program invocation, it is unlikely that this code path will execute at all,
    // because most likely there will be either a flag file (which causes a return above) or key
    // file present. The worst-case scenario is that the user sees help text that they don't need,
    // so this is a sufficiently rigorous tracking method.
    static FIRST_RUN: AtomicBool = AtomicBool::new(true);

    if FIRST_RUN.swap(false, Ordering::SeqCst) {
        println!(
            // Wrapped to 80 characters.
            "Sira supports optional (but highly recommended) cryptographic signing of\n\
            manifest and task files as well as actions sent from the control node to\n\
            sira-client on managed nodes. This prevents an attacker who gains access to\n\
            Sira on any of the protected nodes from leveraging Sira to compromise your\n\
            systems or network. However, you will need to use ssh-keygen to sign your\n\
            manifest and task files, or Sira will refuse to run them.\n\
            \n\
            The manifest key signs manifest and action files, allowing the control node\n\
            to reject unauthorized instructions. (Present: {manifest})\n\
            \n\
            The action key signs actions sent from the control node to sira-client,\n\
            allowing managed nodes to reject unauthorized instructions. (Present: {action})\n\
            \n\
            Both keys are independent: you may freely set neither, either, or both.\n\
            \n\
            Protecting these keys with strong and unique passwords is highly recommended.\n",
            manifest = key_exists(allowed_signers_dir(), MANIFEST_KEY),
            action = key_exists(key_dir(), ACTION_KEY),
        );
    }

    print!(
        "Do you wish to generate and deploy the {} key? [Y/n]: ",
        key_name.as_ref().display(),
    );
    io::stdout().flush().expect("could not flush stdout");
    if !io::stdin()
        .lines()
        .next()
        .unwrap()
        .unwrap()
        .trim()
        .to_ascii_lowercase()
        .starts_with('n')
    {
        println!();
        ssh_keygen(dir.as_ref().join(key_name));
        true
    } else {
        let _ = File::create(flag_file).expect("could not create flag file");
        false
    }
}

/// Transforms a private key file name to a public key file name.
///
/// Appends ".pub" without performing a lossy UTF-8 conversion.
fn public_key(private_key: impl AsRef<Path>) -> PathBuf {
    let mut key_path: OsString = private_key.as_ref().to_owned().into();
    key_path.push(".pub");
    key_path.into()
}

/// Runs `ssh-keygen` to generate a key pair suitable for Sira.
///
/// `key_file` is the path to the private key.
///
/// Panics if this process exits with an error.
fn ssh_keygen(key_file: impl AsRef<OsStr>) {
    client::run(
        "ssh-keygen",
        &[
            OsStr::new("-t"),
            OsStr::new("ed25519"),
            OsStr::new("-C"),
            OsStr::new("sira"),
            OsStr::new("-f"),
            key_file.as_ref(),
        ],
    )
    .expect("could not generate SSH login key pair");
    println!();
}
