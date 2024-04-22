//! An installer for sira-client.
//!
//! TODO Move the invocation workflow to a user-facing document and update this doc to point there.
//!
//! TODO Remove the "text" in "```text" after converting to this to standalone GFM.
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

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if todo!() {
        managed_node();
    } else {
        control_node();
    }
}

fn managed_node() {
    // Extract and verify required command-line arguments.
    //
    // Tentative signature:
    //
    // sira-install --managed-node <sira-user>
    //
    // If the flag in these arguments is documented at all, make it clear that it is for internal
    // use only. To enlist localhost as a managed node, run sira-install normally against
    // localhost; don't try to use this flag.

    // Copy sira.pub to the Sira user's ~/.ssh/authorized_keys, ensuring correct permissions.
    // Feel free to assume it's at /home/<sira-user>. If someone wants to deploy this in a funky
    // setup, they can write their own installer or modify this one; this is all well-documented.

    // Ensure existence of /opt/sira/bin. Don't mangle the administrator's owner, group, or
    // permissions: by default, this operation should require root, but if the
    // administrator is doing something different, we'll trust them to know what they're doing.

    // Move sira-client to /opt/sira/bin/sira-client. Ensure correct user, group, & permissions.

    // Install the Sira user in sudoers, idempotently.

    // Ensure the existence of the /etc/sira directory structure.

    // If present in the CWD, install the action allowed_signers file. Remember to move the
    // identity from the end to the start. Prompt to replace (?).

    // Check for unexpected permissions and warn, in case the user made a mistake.
}

fn control_node() {
    // Extract and verify required command-line arguments.
    //
    // Tentative signature:
    //
    // sira-install <sira-user> [<admin-user>@]<managed-node> [ssh-option ...]
    //
    // where ssh-options are directly passed along to scp and ssh.
    //
    // Note that there is a clear injection opportunity here with untrusted inputs. There is no
    // good way to prevent ssh from interpreting malicious options as commands to run on the target
    // system, AFAIK.

    // Prompt user for consent, and then generate SSH key pairs, if the files don't already exist:
    //  - sira (for login)
    //  - manifest
    //  - action
    //
    // If they do exist, use them without prompting. The user will see this for every single
    // managed node, so we should make repeated invocations as effortless and painless as possible.
    //
    // If they don't exist and the user declines to create them, touch some config files to store
    // that preference, e.g. .sira-skip-manifest-key and .sira-skip-action-key.

    // Ensure that the /etc/sira directory structure exists, and write the control node files.
    // Don't overwrite them if they already exist. Don't remove the ones in the current directory
    // or you'll mess up the steps above on the next run.

    // Transfer files to managed node via SCP:
    //  - sira-install (required)
    //  - sira-client  (required)
    //  - sira.pub     (optional)
    //  - action.pub   (optional)

    // SSH over to the managed node using the user@host from the command-line arguments. Run:
    //
    // sudo ./sira-install -
    //
    // Be sure to use std::process::Command::new("ssh") rather than the openssh crate, because we
    // specifically WANT stdio to be piped to enable password-protected sudo in this case.
    //
    // TODO Verify that this actually works with password-protected sudo.
}
