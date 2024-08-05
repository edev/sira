# Installing Sira

Below are step-by-step instructions for installing Sira across both your control node and your managed nodes. The guide is long, but using the installer allows you to skip most steps. After initial reading, it should only take a minute or two per node with the installer or around 5-10 minutes per node (depending on your speed with the terminal) for manual installation.

First, some important terms:
- **Control node:** the computer or virtual machine that controls everything. This is where you will run `sira` to manage your systems.
- **Control node user:** the user account on the control node where you will install and run `sira`. If you wish to use the installer, this account must be a sudoer.
- **Managed node:** a computer or virtual machine that Sira administers, i.e. one you want to be able to list in manifest files.
- **Managed node admin:** the account on a managed node that you will use for setting up Sira. If you wish to use the installer, this account must be accessible via SSH and must be a sudoer.
- **Sira user:** the account on managed nodes that runs `sira-client`. This account needs password-less sudo access to `sira-client` and should have no other special access.

## Choosing a control node & user

If your security threat profile is low, it's probably fine to choose your primary PC as your control node and any trusted, non-root account as the control node user, as long as you use password-protected manifest and action keys. (You will set these up below. See [security.md](security.md) for much more information on these keys.) Of course, choosing a more secure setup, e.g. creating a separate user account for Sira, is always a good idea, if you are willing and able.

If your threat profile is higher, you might want to create a control node virtual machine that you only start when you need it or even use a dedicated, physical machine as the control node. Separating all aspects of your Sira configuration from any anticipated threat vectors will substantially improve your security posture.

**Remember: if you want to use the installer, the control node user must be a sudoer.**

## Initial steps

As the control node user:

1. Install [rustup](https://rustup.rs/).
1. Rustup will probably handle this, but just in case it doesn't: add `~/.cargo/bin` to your `PATH`, and then restart your terminal.
1. Compile Sira's control node and client binaries (Sira is small, so this should be quick):

```
git clone git@github.com:edev/sira.git
cargo install --path sira
```

### Static vs. dynamic linking

If you intend to manage very similar Linux systems, e.g. all running fully updated versions of the same distribution, the instructions above will likely be fine. Otherwise, you might need to build statically linked binaries, especially `sira-client`:

```
rustup target add x86_64-unknown-linux-musl
cargo install --target x86_64-unknown-linux-musl --path sira
```

I readily admit that the full implications of this change are beyond my expertise (at time of writing). However, `x86_64-unknown-linux-musl` is a [tier 2](https://doc.rust-lang.org/nightly/rustc/platform-support.html) target, guaranteed to build but not guaranteed to work. It should generally be well-supported, and Sira is quite simple, but there's no guarantee. If you run into a problem, please open an issue! Additionally, if you know more than I do, please feel free to open an issue to discuss how best to handle and document this. Thank you!

## Automatic installation (recommended)

For each managed node:
1. Ensure that the control node user has an approved SSH fingerprint for the managed node. The easiest way to do this is simply to SSH from the control node user to the managed node admin in preparation for the next step.
1. Create the Sira user on the managed node. Since different distributions handle account creation slightly differently, the installer does not automate this, and we do not provide instructions here. The Sira user needs no special permissions or configuration of any kind: the more bare-bones the account, the better. Do not configure key-based SSH login, as the installer will do this for you.

    The Sira user should be a separate account used only for Sira, for two reasons. First, this is best practice in general. Second, the installer will install a sudoers rule granting this account password-less sudo access to `sira-client` and nothing else; if you try to use an existing admin account, the installer will lock this account out of sudo! (You could then hypothetically fix this with Sira, if you were sufficiently determined.)

    If you need even tighter security, you are free to use different user names for different managed nodes, as long as you configure them in your control node user's `~/.ssh/config` file. This should work fine with the installer. You are also free to use different login SSH keys for each managed node, but the installer does not support this. Sira does not support using different manifest and action keys for each managed node.
1. As the control node user, run:

    ```
    $ sira-install <sira-user> <managed-node-admin>@<managed-node>
    ```

    If you need to set other options, such as port number or identity file, you will need to set them through `~/.ssh/config`. Because various OpenSSH utilities use different formats for specifying these options, `sira-install` does not accept options to pass through.

    The first time you run `sira-install`, it will configure the control node and then the managed node. Subsequent invocations will be much simpler, since the control node will already be configured.

In summary, to install Sira on a node, run the following from the control node user:

```
# Add the Sira user to the managed node. Also, store the managed node's SSH fingerprint.
ssh -t <managed-node-admin>@<managed-node> sudo useradd [options] <sira-user>

# Install Sira on the managed node. On first run, sets up the control node, too.
sira-install <sira-user> <managed-node-admin>@<managed-node>
```

### Final Steps

1. Transfer the manifest private key (e.g. `~/.ssh/manifest`) from the control node user to the account where you will develop your manifests and tasks. Remove it from the control node user.
1. Make note of the following invocation. When you wish to sign your manifest and task files, you will need to run the following (which you might want to put in a simple script on your development machine):

    ```
    ssh-keygen -Y sign -n sira -f <path-to-key> <file-name> ...
    ```

    Example script:

    ```
    #!/bin/bash

    set -e

    # For ease of use, consider starting ssh-agent on login and using
    # AddKeysToAgent in ~/.ssh/config

    ssh-keygen -Y sign -n sira -f ~/.ssh/manifest "$@"
    ```
1. As the control node user, configure ~/.ssh/config to meet the following requirements. (How you do this will vary depending on your setup.)
    1. When connecting to managed nodes as the Sira user, use the login key (e.g. `~/.ssh/sira`).
    1. If possible, when connecting to managed nodes, log in as the Sira user by default. If you don't set this property, you will need to write `<user>@<host>` instead of simply `<host>` in all of your manifest files.

    In the simplest case, if the control node user's only job is running Sira, you might be able to get away with a wildcard:

    ```
    Match host *
      User sira
      IdentityFile ~/.ssh/sira
    ```

    To test your configuration, try connecting to one or more managed nodes using either `ssh <host>` or `ssh <user>@<host>`, depending on the above. (Remember to use `ssh-add` to add the key to `ssh-agent` first.) You should be able to log in as the Sira user without being prompted for any information.
1. Verify that everything is working. Write the following files as the control node user:

    ```
    # ~/manifest.yaml
    ---
    name: Hello World
    hosts:
      # Insert your managed nodes here
      - <managed node>
      - <managed node>
      ...
    include:
      - hello_world.yaml

    # ~/hello_world.yaml
    ---
    name: Hello World
    actions:
      - command:
          - echo "Hello, world!"
    ```

    Then, as your control node user, from its home directory, run:

    ```
    $ sira manifest.yaml
    ```

You're done! Congratulations! If you're looking to further automate and elevate your configuration management, creating a Git repository for your Sira manifests and tasks, creating Git repositories for per-user configuration ("dotfiles repos"), and using a tool like [GNU Stow](https://www.gnu.org/software/stow/).

## Manual installation: control node

1. If you haven't already gone through the sections of this document *above* Automatic Installation, please do so now.
1. Open [security.md](security.md) and scroll down to the table listing files and permissions. (Reading the file is also a good idea, of course.) Use this table as your reference for Sira's configuration files throughout this guide; we will refer to it as "the table."
1. If you haven't already created `~/.ssh`, create it now.
1. Generate the SSH key pair that Sira will use to log into managed nodes (as the Sira user). This guide will assume that it the private key is  `~/.ssh/sira` and the public key is `~/.ssh/sira.pub`. Protecting all SSH keys used in this guide with strong and unique passwords is highly recommended but not required.

    ```
    $ ssh-keygen -t ed25519 -C "sira@<domain>" -f ~/.ssh/sira
    ```

    You are free to customize this key and its comment field any way you wish. Sira does not directly use this key; you will configure OpenSSH to use this key later in the guide.
1. *(Optional but highly recommended)* Generate SSH keys to cryptographically sign your manifest and task files, as well as actions sent from the control node to managed nodes, providing much stronger protection against unauthorized access. For more information, see [security.md](security.md).
    1. Generate the keys:

        ```
        $ ssh-keygen -t ed25519 -C sira -f ~/.ssh/manifest
        $ ssh-keygen -t ed25519 -C sira -f ~/.ssh/action
        ```

        Note that the comment field **must** be exactly `sira`, as specified above.
    1. Create the directories specified in the table. Please remember to set the owner, group, and permissions according to the table as well.
    1. Install the manifest allowed signers file:
        1. As root, copy the manifest public key to `/etc/sira/allowed_signers/manifest`
        1. As root, modify the newly created file. Move the last field in the file to the beginning of the file. (For more information on this format, see `man ssh-keygen`.)

            For example, the sample public key:

            ```
            ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOLjDP1zGmwWiaaW1i2z/GpVCSb6xLtCvkJtW/yu8dFO sira
            ```

            Becomes:

            ```
            sira ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOLjDP1zGmwWiaaW1i2z/GpVCSb6xLtCvkJtW/yu8dFO
            ```
    1. Install the action key files, e.g.:

        ```
        $ sudo cp ~/.ssh/action ~/.ssh/action.pub /etc/sira/keys
        ```
    1. Set and verify the owner, group, and permissions on all of these newly installed files according to the table. This is **very important**, as they are almost certainly different from your system's defaults.

## Manual installation: managed nodes

### Preparation
1. If you haven't already configured the control node as described above, please do so now.
1. Work through the Automatic Installation section, but stop when instructed to run `sira-install`.
1. Transfer the following files from the control node user to the managed node admin:
    - The client binary (e.g. `~/.cargo/bin/sira-client`)
    - The login public key (e.g. `~/.ssh/sira.pub`)
    - The action public key (e.g. `~/.ssh/action.pub`), if you created one

    For example:

    ```
    $ scp ~/.cargo/bin/sira-client ~/.ssh/sira.pub ~/.ssh/action.pub <destination>:
    ```
1. Log into the managed node admin.

### As the managed node admin
1. Move `sira.pub` to the Sira user's `~/.ssh/authorized_keys`. (You will likely need to create the directory first.) Remember to set ownership and permissions sensibly.
1. As root, create `/opt/sira/bin`, e.g.:

    ```
    $ sudo mkdir -p /opt/sira/bin
    ```

    Please note that you **must** use this path. Sira does not support alternate installation locations for `sira-client` at this time.
1. Set the owner (`root:root`) and permissions (`700`) on `sira-client` and move it to `/opt/sira/bin`, e.g.:

    ```
    $ sudo chown root:root sira-client
    $ sudo chmod 700 sira-client
    $ sudo mv sira-client /opt/sira/bin
    ```

    **It is vitally important that you set ownership and permissions correctly on sira-client!**
1. Grant the Sira user password-less sudo access to `/opt/sira/bin/sira-client` by adding the following to the appropriate sudoers file for your system:

    ```
    <sira-user>	ALL=(root:root) NOPASSWD:/opt/sira/bin/sira-client
    ```

    Note: if granting password-less sudo access makes you queasy, good! It should! This is why Sira supports cryptographically signing both the instructions that the control node program `sira` reads and the instructions that it sends to `sira-client`, rendering this sudo access mostly meaningless unless those keys are compromised (or a vulnerability in `sira-client` compromises this protection).
1. If you generated an action key, install the action allowed signers file to `/etc/sira/allowed_signers/action`:
    1. Create the directories specified in the table. Please remember to set the owner, group, and permissions according to the table as well.
    1. As root, copy the action public key to `/etc/sira/allowed_signers/action`
    1. As root, modify the newly created file. Move the last field in the file to the beginning of the file. (For more information on this format, see `man ssh-keygen`.)

        For example, the sample public key:

        ```
        ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOLjDP1zGmwWiaaW1i2z/GpVCSb6xLtCvkJtW/yu8dFO sira
        ```

        Becomes:

        ```
        sira ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOLjDP1zGmwWiaaW1i2z/GpVCSb6xLtCvkJtW/yu8dFO
        ```
1. Set and verify the owner, group, and permissions on this file according to the table.
1. Remove the action public key and the installer from the managed node admin.

Once you're done installing across all managed nodes, return to the Automatic Installation section and go through the Final Steps.

# Uninstalling

Uninstalling Sira is a straightforward but manual process. Below are the changes you will need to make.

## Control node

Remove the following:

| Resource            | Description                     |
| :------------------ | :------------------------------ |
| /etc/sira           | Sira's configuration directory  |
| /home/\<sira-user\> | Sira user's home directory      |
| The Sira user       | The user account that runs Sira |

If you do not wish to remove the Sira user, you may instead remove the following within that user's home directory:

| Resource                                      | Description                                       |
| :-------------------------------------------- | :------------------------------------------------ |
| ~/.cargo/bin/{sira,sira-client,sira-install}  | Sira binaries. To remove: `cargo uninstall sira`  |
| ~/.ssh/{sira,sira.pub}                        | Client access keys                                |
| ~/.ssh/{manifest,manifest.pub}                | Manifest key (private key might not be present)   |
| ~/.ssh/{action,action.pub}                    | Action key                                        |

# Managed nodes

Remove the following:

| Resource            | Description                                   |
| :------------------ | :-------------------------------------------- |
| /etc/sira           | Sira's configuration directory                |
| /opt/sira           | Deployment directory for `sira-client`        |
| Sudoers entry       | Either in `/etc/sudoers` or `/etc/sudoers.d`  |
| /home/\<sira-user\> | Sira user's home directory                    |
| The Sira user       | The user account that runs Sira               |

If you do not wish to remove the Sira user, you may instead remove the following within that user's home directory:

| Resource                | Description               |
| :---------------------- | :------------------------ |
| ~/.ssh/authorized\_keys | Client access public key  |
