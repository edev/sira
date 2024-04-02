# Securing Sira

Sira's security comes from a combination of OpenSSH's security and the architecture of Sira itself. Fully securing your OpenSSH installations across your network is beyond the scope of Sira's documentation, but Sira requires that you use SSH keys to log into managed nodes, which is a strong security practice for any OpenSSH installation. Sira's security architecture is described below.

# Logging into managed nodes

Sira logs into managed nodes the same way you would log into them yourself: it asks OpenSSH to connect to the target system, and your OpenSSH configuration does the rest. Sira deliberately disallows password-based SSH authentication, leaving the more secure key-based authentication as the logical choice for most users. (In Sira's documention, we refer to this as the **client access key**.) However, you are free to configure your nodes to use some other form of authentication within OpenSSH if you prefer.

# Running actions on managed nodes

For actions that OpenSSH can execute directly, Sira does exactly that. For instance, if you need to upload or download a file, Sira simply uses OpenSSH file transfer. Actions such as running shell commands and modifying file lines, however, require the use of a client application, `sira-client`, on the managed node.

`sira-client` must be owned by `root:root` with `0700` permissions. As part of bootstrapping a node, you must give the user as whom Sira logs in (we'll call this **the Sira user**) permission to run `sira-client` via `sudo` without requiring a password. This is the only shell command that the control node runs directly on managed nodes; `sira-client` handles the rest. The Sira user requires no other special permissions; for instance, it *does not* need unrestricted `sudo` access.

Funneling all local client actions through `sira-client` has at least two important security benefits. First, it provides access control: `sira-client` can verify that requested actions are actually authorized. (This is optional and is described below.) Second, it opens the smallest possible window through which to access administrative functions on managed nodes.

Two alternatives to this scheme were considered and rejected:

1. Require the system administrator to grant the Sira user unrestricted and passwordless `sudo` access to the entire managed node. This is the obvious and easy way to set up a remote administration system. However, this approach also enables an attacker who gains even momentary access to the Sira user to compromise the managed node.

2. Require the system administrator to grant the Sira user `sudo` access to the entire managed node, but configure `sudo` to require a password. This approach provides some security against unauthorized access, but it also creates several opportunities for an attacker to compromise a sudoer's password.

# Preventing unauthorized use of `sira-client`

Since `sira-client` always runs as root and knows how to perform potentially harmful system administration tasks, securing it against unauthorized use is of paramount importance. For this, Sira uses optional, but highly recommended, SSH key signing. In short, Sira supports cryptographically signing manifest and task files and verifying these signatures on both the control node and managed nodes. Details are below.

When Sira processes a list of manifest files on the control node, it generates and executes a sequence of actions for each managed node. When the control node needs to invoke `sira-client` on a managed node, it uses the **action key** to cryptographically sign each action and sends both the action and the signature to `sira-client` on the managed node. `sira-client` then uses the corresponding public key to verify the action before running it. If the public key is installed on a managed node (in the form of an OpenSSH allowed signers file), `sira-client` will refuse to run unsigned or improperly signed actions. Similarly, if `sira-client` receives a signed action but does not have a public key installed, it will exit with an error instructing the administrator to install the public key.

Stepping backwards in the chain of trust, Sira supports signing manifest and task files with a **manifest key**. The system administrator can develop manifest and task files in a test environment, sign the files, and transfer them to the control node (perhaps by committing them to source control). On the control node, Sira will see these signatures and verify them against the corresponding public key, following the same logic described above.

Thus, if both keys are present and properly protected (e.g. by passwords), both the control node and managed nodes will refuse to execute instructions from unauthorized parties, even in the event that an attacker gains access to these nodes.

# File locations

```
# Manifest private key (only on development machine)
#
# Manage this key the same way as your other SSH keys on your development user account. Sira does
# not interact directly with this key. The following is merely a suggestion.
~/.ssh/sira/manifest

# Manifest key allowed signers file (only on control node)
/etc/sira/allowed_signers/manifest

# Action private key (only on control node)
/etc/sira/keys/action

# Action key allowed signers file (only on managed nodes)
/etc/sira/allowed_signers/action
```

# Notes on file permissions

Sira does not mandate a specific approach to securing the contents of `/etc/sira`. You are free to implement whatever security scheme works best for you. However, Sira does apply certain defaults when asked to bootstrap a node; these are listed below. These are also the minimal permissions required for Sira to run.

| File or directory                   | Description                    | Nodes   | Owner:group        | Permissions |
| :---------------------------------- | :----------------------------- | :------ | :----------------- | :---------- |
| /etc/sira/                          | Sira configuration directory   | Both    | root:\<sira-user\> | 0050        |
| /etc/sira/allowed\_signers/         | Allowed signers directory      | Both    | root:\<sira-user\> | 0050        |
| /etc/sira/allowed\_signers/action   | Authorizes action public key   | Managed | root:\<sira-user\> | 0040        |
| /etc/sira/allowed\_signers/manifest | Authorizes manifest public key | Control | root:\<sira-user\> | 0040        |
| /etc/sira/keys/                     | Sira SSH key directory         | Both    | root:\<sira-user\> | 0050        |
| /etc/sira/keys/action               | Action private key             | Control | root:\<sira-user\> | 0040        |
| /etc/sira/keys/action.pub           | Action public key              | Control | root:\<sira-user\> | 0040        |
