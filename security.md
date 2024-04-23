# Securing Sira

Sira's security comes from a combination of OpenSSH's security and the architecture of Sira itself. Fully securing your OpenSSH installations across your network is beyond the scope of Sira's documentation, but Sira requires that you use SSH keys to log into managed nodes, which is a strong security practice for any OpenSSH installation. Sira's security architecture is described below.

# Logging into managed nodes

Sira logs into managed nodes the same way you would log into them yourself: it asks OpenSSH to connect to the target system, and your OpenSSH configuration does the rest. Sira deliberately disallows password-based SSH authentication, leaving the more secure key-based authentication as the logical choice for most users. (In Sira's documention, we refer to this as the **client access key**.) However, you are free to configure your nodes to use some other form of authentication within OpenSSH if you prefer, as long as Sira on the control node can access managed nodes without requiring a password at runtime.

# Running actions on managed nodes

For some actions, Sira's workflow involves running commands on the control node. For instance, if you need to upload a file, Sira starts by using OpenSSH file transfer to copy the file from the control node to a temporary file on each managed node. At some point, though, actions typically require the use of a client application, `sira-client`, on the managed node.

`sira-client` must be owned by `root:root` with `0700` permissions. As part of bootstrapping a node, you must give the user as whom Sira logs in (we'll call this **the Sira user**) permission to run `sira-client` via `sudo` without requiring a password. This is the only shell command that the control node runs directly on managed nodes; `sira-client` handles the rest. The Sira user requires no other special permissions; for instance, it *does not* need unrestricted `sudo` access.

```
# Example configuration (edit with visudo):

# Allow <user> to run /opt/sira/bin/sira-client as root:root without entering a password.
# Note that sudoers applies the last matched entry. If this user is already a sudoer, modify the existing entry instead.
<user>  ALL=(root:root) NOPASSWD:/opt/sira/bin/sira-client
```

Funneling all local client actions through `sira-client` has at least two important security benefits. First, it provides access control: `sira-client` can verify that requested actions are actually authorized. (This is optional and is described below.) Second, it opens the smallest possible window through which to access administrative functions on managed nodes.

Two alternatives to this scheme were considered and rejected:

1. Require the system administrator to grant the Sira user unrestricted and passwordless `sudo` access to the entire managed node. This is the obvious and easy way to set up a remote administration system. However, this approach also enables an attacker who gains even momentary access to the Sira user to compromise the managed node.

2. Require the system administrator to grant the Sira user `sudo` access to the entire managed node, but configure `sudo` to require a password. This approach provides some security against unauthorized access, but it also creates several opportunities for an attacker to compromise a sudoer's password.

# Preventing unauthorized use of `sira-client`

Since `sira-client` always runs as root and knows how to perform potentially harmful system administration tasks, securing it against unauthorized use is of paramount importance. For this, Sira uses optional, but highly recommended, SSH key signing. In short, Sira supports cryptographically signing manifest and task files and verifying these signatures on both the control node and managed nodes. Details are below.

When Sira processes a list of manifest files on the control node, it generates and executes a sequence of actions for each managed node. When the control node needs to invoke `sira-client` on a managed node, it uses the **action key** to cryptographically sign each action and sends both the action and the signature to `sira-client` on the managed node. `sira-client` then uses the corresponding public key to verify the action before running it. If the public key is installed on a managed node (in the form of an OpenSSH allowed signers file), `sira-client` will refuse to run unsigned or improperly signed actions. Similarly, if `sira-client` receives a signed action but does not have a public key installed, it will exit with an error instructing the administrator to install the public key.

Stepping backwards in the chain of trust, Sira supports signing manifest and task files with a **manifest key**. The system administrator can develop manifest and task files in a test environment, sign the files, and transfer them to the control node (perhaps by committing them to source control). On the control node, Sira will see these signatures and verify them against the corresponding public key, following the same logic described above.

Thus, if both keys are present and properly protected (e.g. by passwords), both the control node and managed nodes will refuse to execute instructions from unauthorized parties, even in the event that an attacker gains access to these nodes.

# File locations and permissions

The files that Sira uses for signing and verifying manifests, tasks, and actions are listed in the table below. Sira does not mandate a specific approach to securing the contents of `/etc/sira`; you are free to implement whatever security scheme works best for you. However, Sira does apply certain defaults when asked to bootstrap a node (NYI); these are listed below.

| File or directory                   | Description                    | Nodes   | Owner:group        | Permissions |
| :---------------------------------- | :----------------------------- | :------ | :----------------- | :---------- |
| /etc/sira/                          | Sira configuration directory   | Both    | root:\<sira-user\> | 0755        |
| /etc/sira/allowed\_signers/         | Allowed signers directory      | Both    | root:\<sira-user\> | 0755        |
| /etc/sira/allowed\_signers/action   | Authorizes action public key   | Managed | root:\<sira-user\> | 0644        |
| /etc/sira/allowed\_signers/manifest | Authorizes manifest public key | Control | root:\<sira-user\> | 0644        |
| /etc/sira/keys/                     | Sira SSH key directory         | Control | root:\<sira-user\> | 0755        |
| /etc/sira/keys/action               | Action private key             | Control | root:\<sira-user\> | 0640        |
| /etc/sira/keys/action.pub           | Action public key              | Control | root:\<sira-user\> | 0644        |

The one key not listed above is the **manifest private key**, which belongs on the development machine. You are free to manage and secure this key alongside your other SSH keys.
