//! An installer for sira-client.
//!
//! TODO Try to statically link all binaries. Verify with the `file` utility. Examine what's
//! dynamically linked and how it's licensed.
//!
//! TODO Move the invocation workflow to a user-facing document and update this doc to point there.
//!
//! TODO Remove the "text" in "```text" after converting to this to standalone GFM.
//!
//! TODO Check for files left behind by installer and clean them up. Flags don't count.
//!
//! # Installation Guide
//!
//! Below are step-by-step instructions for installing Sira across both your control node and your
//! managed nodes. The guide is long, but it should only take a few minutes on the control node and
//! about a minute per managed node.
//!
//! 1. On the control node, as the user who will run Sira:
//!     1. Install [rustup](https://rustup.rs/).
//!     1. Compile Sira's control node and client binaries: `cargo install sira`
//!     1. Generate the Sira user's SSH login key pair:
//!
//!        ```text
//!        $ ssh-keygen -t ed25519 -C "sira@<domain>"
//!        ```
//!
//!        Protecting this key pair with a password is highly recommended but not required.
//!     1. *(Optional but recommended)* Generate SSH keys to crypographically sign your manifest
//!        and task files, providing much stronger protection against unauthorized access.
//!
//!         1. Generate the key pairs:
//!
//!            ```text
//!            $ ssh-keygen -t ed25519 -C sira -f manifest
//!            $ ssh-keygen -t ed25519 -C sira -f action
//!            ```
//!
//!            Protecting these key pairs with unique passwords is highly recommended but not required.
//!         1. Transfer the manifest private key to the account where you will develop your
//!            manifests and tasks. Remove it from the control node.
//!         1. Make note of the following invocation. When you wish to sign your manifest and task
//!            files, you will need to run the following (which you might want to put in a simple
//!            script on your development machine):
//!
//!            ```text
//!            ssh-keygen -Y sign -n sira -f <path-to-key> <file-name> ...
//!            ```
//!
//! 1. For each managed node:
//!    TODO Rewrite this after seeing how much I can automate.
//!     1. On the control node, transfer `sira-install` and `sira-client` to a user who can run tasks
//!        as root on the managed node, e.g.:
//!
//!        ```text
//!        $ scp ~/.cargo/bin/sira-install ~/.cargo/bin/sira-client <user>@<node>:
//!        ```
//!
//!        If your managed nodes disallow password-based SSH, you might want to transfer the Sira
//!        user's public SSH key to the same user so that you can install it below. (You can also
//!        use Sira to disable password-based SSH later, e.g. via [Action::LineInFile].)
//!
//!     1. On the managed node:
//!         1. Add the Sira user.
//!
//!            **Important** It is highly recommended that you use a dedicated user for Sira. However, if you intend to use an existing user with sudo access, please be aware that `sira-install` will likely break sudo for this user. This is because `sira-install` will add a line at the end of the sudoers file that only grants the Sira user sudo access to `sira-client`, and the last matching entry in a sudoers file wins. You will need to edit your sudoers file after running `sira-install`, or you may opt to skip `sira-install` and perform the same steps manually. They are documented below and in [security.md](security.md).
//!         1. Run the installer as root:
//!
//!            ```text
//!            # sira-install <sira-user>
//!            ```
//!
//!            This program performs the following actions;
//!             - Moves `sira-client` to `/opt/sira/bin`, setting ownership and permissions.
//!             - TODO Complete this list.
//!     1. Back on the control node, deploy the Sira user's SSH public key, e.g.:
//!
//!        ```text
//!        scp ~/.ssh/<sira-key>.pub <sira-user>@<node>:
//!        ```
//!
//! 1. On the control node, write a ~/.ssh/config entry that will direct Sira to connect to the
//!    proper user and use the proper key when connecting to managed nodes.
//!
//! [Action::LineInFile]: sira::core::Action::LineInFile

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

// FIle names of the installer and sira-client binaries.
const INSTALLER_BIN: &str = "sira-install";
const CLIENT_BIN: &str = "sira-client";

// The installation directory for sira-client.
const CLIENT_INSTALL_DIR: &str = "/opt/sira/bin";

// The path of the file where SSH keys are stored on both control and managed nodes. This path is
// relative to the user's home.
const SSH_DIR: &str = ".ssh";

// Keys used to log in via SSH as the Sira user on managed nodes.
const LOGIN_KEY: &str = "sira";

// Keys used to sign and verify manifest and task files, known as the "manifest key".
const MANIFEST_KEY: &str = "manifest";

// Keys used to sign and verify generated actions sent to sira-client, known as the "action key".
const ACTION_KEY: &str = "action";

// File names, in the CWD, of flag files for the manifest and action keys. If the corresponding key
// files don't exist, but these flags do, this indicates that we have already asked prompted the
// administrator to generate these keys during a previous program run and they have declined.
//
// Thus, if the keys are missing but the flag files are present, we will simply skip the keys.
const MANIFEST_FLAG: &str = ".sira-install-skip-manifest-key";
const ACTION_FLAG: &str = ".sira-install-skip-action-key";

// TODO Strongly consider moving this to crypto.rs and deploying globally. Same with key_dir().
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

/// Indicates whether a public key is present as either an allowed signers file (in the Sira
/// configuration directory) or a public key file (in some expected location).
#[derive(Clone, Debug, PartialEq, Eq)]
enum PublicKeyState {
    AllowedSignersFile,
    PublicKeyFile,
    NotPresent,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 3 {
        if args[1] == "--managed-node" {
            managed_node(&args[2]);
        } else {
            control_node(&args[1], &args[2]);
        }
    } else {
        // TODO Write a default help message.
    }
}

fn managed_node(sira_user: &str) {
    // Extract and verify required command-line arguments.
    //
    // Tentative signature:
    //
    // sira-install --managed-node <sira-user>
    //
    // If the flag in these arguments is documented at all, make it clear that it is for internal
    // use only. To enlist localhost as a managed node, run sira-install normally against
    // localhost; don't try to use this flag.

    // Move sira.pub to the Sira user's ~/.ssh/authorized_keys, ensuring correct permissions.
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

    // Ensure existence of /opt/sira/bin. Don't mangle the administrator's owner, group, or
    // permissions: by default, this operation should require root, but if the
    // administrator is doing something different, we'll trust them to know what they're doing.
    if !path_exists(CLIENT_INSTALL_DIR, "client install directory") {
        println!("Creating {CLIENT_INSTALL_DIR}");
        client::run("mkdir", &["-p", CLIENT_INSTALL_DIR]).expect("error creating directory");
        println!();
    }

    // Move sira-client to /opt/sira/bin/sira-client. Ensure correct user, group, & permissions.
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

    // Ensure the existence of the /etc/sira directory structure.
    create_config_dirs();

    // If present in the CWD, install the action allowed_signers file. Remember to move the
    // identity from the end to the start. If it already exists, do not replace it.
    {
        let cwd = ".";
        let key = ACTION_KEY;
        if key_exists(cwd, public_key(key)) {
            install_allowed_signers_file(cwd, key);
        }
    }
}

fn control_node(sira_user: &str, destination: &str) {
    // Extract and verify required command-line arguments.
    //
    // Tentative signature:
    //
    // sira-install <sira-user> [<admin-user>@]<managed-node>
    //
    // If you need to provide options to the SSH connection, you must do so through other means,
    // such as ~/.ssh/config, because we use both scp and ssh, and they specify options
    // differently. For example, port number is -P on scp and -p on ssh, and scp exits with an
    // error if passed -p.

    // Prompt user for consent, and then generate SSH key pairs, if the files don't already exist:
    //  - sira (for login)
    //  - manifest
    //  - action
    //
    // If they do exist, use them without prompting. This program is run once for every managed
    // node, so we should make repeated invocations as effortless and painless as possible.
    //
    // If they don't exist and the user declines to create them, touch some config files to store
    // that preference, e.g. .sira-install-skip-manifest-key and .sira-install-skip-action-key.

    // Compute the Cargo bin directory, typically ~/.cargo/bin
    let cargo_bin_dir = {
        let mut cargo_home = home::cargo_home().expect("could not retrieve Cargo directory");
        cargo_home.push("bin");
        cargo_home
    };

    // Compute the user's home directory.
    let home_dir = home::home_dir().expect("could not retrieve user's home directory");

    // List of files to transfer to managed node (in a moment).
    let mut file_transfers = vec![
        cargo_bin_dir.join(INSTALLER_BIN),
        cargo_bin_dir.join(CLIENT_BIN),
    ];

    // Compute the user's SSH key directory, i.e. ~/.ssh
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

        let installed = if private_installed && public_installed {
            // The action key is already installed on the control node. Select it for deployment to
            // managed nodes.
            true
        } else if private_exists && public_exists {
            // The action key is not installed but is present in ~/.ssh. Install it, and select it
            // for deployment to managed nodes.
            true
        } else {
            // Prompt to generate. If the user consents, then install it and select it for
            // deployment to managed nodes. If the user declines, then set a flag file to remember
            // the user's choice.
            let key_created =
                prompt_to_generate_signing_key_pair(&ssh_dir, ACTION_KEY, ACTION_FLAG);
            if key_created {
                install_signing_key_pair(&ssh_dir, ACTION_KEY);
            }
            key_created
        };
        if installed {
            file_transfers.push(key_dir().join(public_key(ACTION_KEY)));
        }
    }

    {
        // Transfer files to managed node via SCP:
        //  - sira-install (required)
        //  - sira-client  (required)
        //  - sira.pub     (required)
        //  - action.pub   (optional)
        //
        // Invocation;
        // scp <file_transfers> <destination>
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

    // SSH over to the managed node using the user@host from the command-line arguments. Run:
    //
    // ssh -t [<user>@]<host> sudo ./sira-install --managed-node <sira-user>
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
/// if the public key is in both locations, the allowed signers file takes precedence.
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

/// Create the Sira config directory structure if it doesn't exist.
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

/// Checks whether a key exists, panicking if we can't determine an answer.
///
/// This is a wrapper around [Path::try_exists]. It applies key-file-specific logic and error text.
fn key_exists(dir: impl AsRef<Path>, key_name: impl AsRef<Path>) -> bool {
    let path = dir.as_ref().join(key_name.as_ref());
    path_exists(path, "key file")
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
