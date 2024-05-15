# Sira: Simple Remote Administration

Sira ("SIGH-rah", but pronounce it however you please) is a tool for managing small collections of Linux computers (including virtual machines).

By focusing on small, simple deployments, Sira can favor ergonomics, readability, and obvious correctness. By only supporting Linux, Sira can integrate beautifully and natively into Linux workflows.

## Getting started

To get a feel for Sira, keep reading below. Once you're ready to try it out yourself, work through the [installation guide](/installation.md) and return here once you're done.

Sira is split into a control node application called `sira` and a client application called `sira-client`. You pass `sira` a list of files containing instructions, and it connects to nodes via SSH and invokes `sira-client` to execute those instructions. There is no always-on server, and you do not need to open any extra ports. In fact, the account that runs `sira-client` is far better secured than a typical administrator's account. (For details, see [security.md](/security.md).)

### Installing `sira-client` on a managed node

After working through the [installation guide](/installation.md), preparing a managed node to run Sira is as simple as:

```bash
# On the managed node (instructions vary by distribution).
sudo useradd <sira-user>

# On the control node. This should only take a few seconds.
sira-install <sira-user> <managed-node-admin>@<managed-node>
```

That's it!

### Actions

Sira supports a deliberately simple, minimal set of instructions, which Sira calls **actions**:

```yaml
# Run one or more commands on managed nodes (as root).
#
# Note that these processes are created directly and are not interpreted by a shell, so shell
# features like ~ and | will not work. You can always invoke a shell, if you need.
- command:
    - apt-get install -y qemu-system-x86
    - snap install core
    - sudo -u alice bash -c "mkdir -p ~/.ssh"

# For more complex logic, run shell scripts in arbitrary languages and as arbitrary users.
- script:
    name: Install or update Rust
    user: alice

    # Note the "|" after contents. This enables block scalar syntax, which tells Sira to treat the
    # shebang as part of contents and not as a YAML comment.
    contents: |
      #!/bin/bash

      set -e
      cd

      if [[ -e .rustup ]]; then
        source ~/.cargo/env
        rustup update
      else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
      fi

# Replace a line in a file or insert a new line. (This one has lots of advanced options, so please
# read the docs!)
- line_in_file:
    path: /etc/hosts
    line: 192.168.1.4	alice
    after: 127.0.1.1

# Transfer a file from the control node to managed nodes.
- upload:
    # The `from` path is relative to wherever you run `sira`, not the manifest or task file.
    from: files/shared/home/alice/.ssh/authorized_keys
    to:   /home/alice/.ssh/
    user: alice
    group: alice
    permissions: 600
    overwrite: true
```

The design goal for Sira's actions is not to abstract away the details of configuring your systems but to provide a transparent way to perform these same actions across your whole (Linux) network. Performing actions through Sira should look and feel almost exactly the same as performing them by hand in an SSH session.

To access the full documentation, you can do any of the following:
- If Sira is ever uploaded to [crates.io], you will be able to view it at [docs.rs]. Click on the `core` module and then the `Action` enum. (This currently is not the case.)
- Clone this repository, and then run `cargo doc --open` from the repository's directory. Click on the `core` module and then the `Action` enum.
- Browse to [/src/core/action.rs](src/core/action.rs) and read the documentation in source form.

### Tasks

Sira organizes lists of actions as **tasks**. Each task is a YAML document, meaning that it starts with `---`. You can write multiple tasks in a single file, if you prefer, or stick to one task per file. A task groups actions and optionally defines variables for those actions (covered later):

```yaml
---
name: Install system packages
actions:
  - apt-get install $apt_packages
  - snap install core
  - flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  - flatpak install --noninteractive flathub $flathub_bundles
vars:
  # We'll make these much more readable a little later in this guide!
  apt_packages: built-essential git qemu-system-x86
  flathub_bundles: com.discordapp.Discord com.vscodium.codium

---
name: Install Rust and Rust-based apps for alice
actions:
  - script:
      name: Install or update Rust
      user: alice
      contents: |
        #!/bin/bash

        set -e
        cd

        if [[ -e .rustup ]]; then
          source ~/.cargo/env
          rustup update
        else
          curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
        fi
  - script:
      name: Install Rust applications
      user: alice
      contents: |
        #!/bin/bash

        set -e
        cd

        source ~/.cargo/env

        # Cargo seems to recompile and reinstall software unnecessarily when given multiple names.
        # To fix this, provide one name per invocation.
        for name in $crates; do
          cargo install $name
        done
vars:
  crates: bacon comrak
```

### Manifests

Sira groups task files into **manifests** that associate task files with managed nodes (i.e. hosts). Just like tasks, you can write multiple manifests in a manifest file or stick to one per file. Note that you cannot place manifests and tasks in the same file.

```
---
name: debian-base
hosts:
  - alice
include:
  # Paths here are relative to the manifest file.
  - tasks/debian-base.yaml
vars:
  grub_timeout: 10
```

### Variables

As the examples above demonstrated, manifests and tasks can define variables for use in actions by using the `vars` key. When `sira` is about to run an action on a managed node, it compiles a copy of the action to send to `sira-client` as YAML. As part of this process, it substitutes variables into all fields of all actions except Booleans, e.g. `indent` for `line_in_file` and `overwrite` for `upload`. (This is due to a minor technical limitation; if there's demand, applying variables to Boolean fields can be implemented.)

The process is intentionally simple, both so that you don't have to think about complex mechanics and so that Sira can remain agnostic of shell languages, etc. Right before sending an action to a managed node:

1. Sira compiles a dictionary mapping variable names to values.
2. For each variable `v`, Sira searches all fields of the action (except Booleans) for occurrences of `$v` or `${v}` and replaces that text with the variable's value. This consists of one regular expression match-and-replace; it is not recursive.

Task variables override manifest variables, but I am considering reversing this. Best practice for now is to avoid defining a variable in both a task and a manifest that includes the task.

Sira replaces variables in the order in which they were defined. However, depending on this behavior, e.g. to try to create recursive variable substitutions, is a *really bad idea.* It will work, but your files will almost certainly become inscrutable and impossible to maintain!

Variables are not substituted in manifests or in other fields of tasks (e.g. `name`). They are only applied to actions and only in the manner stated above. There is no other "magic."

### Advanced feature: harness the full power of YAML

The choice to use YAML for Sira instead of a more ubiquitous language like JSON is intentional: YAML is a very powerful language with features that can augment your manifests and tasks. (JSON is actually a subset of YAML, so you can technically write JSON instead, if you are sufficiently determined. The docs do not cover this use case.) The `script` action actually depends on an advanced feature of YAML called block scalar syntax, as noted in the examples above.

Using a closely related feature, folded scalar syntax, we can clean up the package lists in the examples above:

```yaml
---
name: Install system packages
actions:
  - apt-get install $apt_packages
  - snap install core
  - flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  - flatpak install --noninteractive flathub $flathub_bundles
vars:
  apt_packages: >-
    built-essential
    git
    qemu-system-x86
  flathub_bundles: >-
    com.discordapp.Discord
    com.vscodium.codium
```

### Advanced feature: use Sira in shell scripts

`sira` works like a standard terminal application: it takes all its inputs as command-line arguments, it keeps running in the foreground until all instructions are complete, it aborts if an action returns a non-zero exit status, and it exits with a non-zero status in the event of a failure. Thus, `sira` integrates cleanly with shell scripts. You can script complex sequences of actions easily, e.g.:

```bash
#!/bin/bash

set -e

sira configure-server.yaml
scp server:files-to-distribute files
sira distribute-files.yaml
```

### Cryptographically sign manifests, tasks, and actions

Sira supports signing manifest and task files as well as actions sent to `sira-client`. If these keys are installed, `sira` will refuse to execute unsigned or improperly signed manifest and task files, and `sira-client` will refuse to execute unsigned or improperly signed instructions. See [security.md](/security.md) for details on how this works and [installation.md](/installation.md) for instructions on setting this up. For msot users, `sira-install` handles this automatically.

If you are using cryptographic signing, you can sign your manifest and task files after changes using OpenSSH's `ssh-keygen`:

```
ssh-keygen -Y sign -n sira -f <path-to-key> <file-name> ...
```

### Run Sira

Once you're ready, running Sira is as simple as adding the relevant SSH keys to your agent and passing your manifest files to `sira`, e.g.:

```bash
# Add client access key
ssh-add ~/.ssh/sira

# Add the action signing key, if installed
ssh-add /etc/sira/keys/action

# Run Sira
sira <manifest-file> ...
```

Sira runs each manifest, task, and action in order; there are no reordering mechanics or dependency graphs. For each managed node, Sira simply runs through its actions as quickly as possible. It *does not* wait for all nodes to complete an instruction before proceeding to the next. If you wish to apply checkpoints, you can write multiple manifest files and call `sira` several times, e.g. in a script (as discussed above).

If a host is unreachable, Sira will ignore it and continue processing other hosts. At the end of the run, `sira` will exit with a `0` exit code signaling success.

If an action fails on any managed node, that node aborts, and the other hosts continue processing. Once the run is complete, `sira` will exit with a non-zero exit code.

## Why not use Ansible, Chef, Puppet, Salt, etc.?

If these tools work well for you, great! Keep using them!

These tools are very sophisticated and very complex. They are designed for use cases as large as enterprise deployments, and their feature sets reflect this. For simple deployments such as homelabs and personal networks, this compexity is often unnecessary. Sira is designed with small-scale, simpler deployments in mind. As a result, Sira makes different choices and different trade-offs.

## Guiding principles

Sira favors simplicity, ergonomics, clarity, and obvious correctness over sophistication and scalability. The goal is not to suit every use case but to provide the best possible experience for users whose needs are simple.

*Please note that these opinions apply specifically to Sira's intended use case and are not meant as criticisms of other projects, especially ones designed for very different use cases! Other projects work differently and have good reasons for doing so.*

Sira's guiding principles are as follows:

* Writing simple instructions should be **simple**. Ideally, it should be as simple as writing them in a shell script or directly in the terminal.

* The way to write complex instructions should be straightforward and obvious.

* It is the job of the system administrator to learn to administer their own systems. Sira's job is to allow the system administrator to express their intent easily, clearly, concisely, and correctly.

* Documentation should be approachable, well-organized, unambiguous, complete, correct, and clear.

* Given the choice of simplicity and ergonomics or advanced features and scalability, prefer simplicity and ergonomics.

* Common terminal tools like grep, apt, and mkdir are assets to be leveraged by the user rather than abstracted away. The only time to duplicate functionality is when doing so either (a) simplifies the task at hand or (b) makes the end result more readable and more obviously correct. Therefore, Sira is designed with the intention that system administration tasks will mostly live off the land.

* The code to create an ergonomic interface for managing a software package, system, or service belongs in that project or its ecosystem rather than in a remote administration tool. In addition, most common system administration tasks now have reasonably convenient interfaces, thanks in part to the prevalence and influence of Ansible, Chef, Puppet, Salt, and so on. Therefore, remote administration plugins are much less valuable than they once were. Meanwhile, eliminating them dramatically reduces attack surface area and supply chain vulnerabilities. Given these trade-offs, and the extremely security-sensitive nature of remote administration tools, Sira does not support plugins.

* Network-connected agents/clients and always-on clients and servers with the ability to arbitrarily modify systems are obvious security liabilities. Tools like OpenSSH already provide plenty of infrastructure for secure communication.

* Code reuse is a good thing. Code should be simple to use, composable, and clear. However, given a choice between composability and simplicity or clarity, prefer simplicity and clarity.

* The user should have full freedom to choose how to organize configuration and supporting files.

* For simple, small-scale use cases, commands should run in the order specified, not based on a dependency graph.

* Errors should be clear, concise, helpful, and caught as early as possible.

## System requirements

Sira is only tested on Linux with 64-bit x86 CPUs. It might work on other Unix-like systems such as MacOS and BSD, and it might work on other architectures like 64-bit Arm, but these are untested. It absolutely **will not** work on Windows.

Since Sira is written in Rust and compiled to binary, it absolutely **will not** manage systems that are binary-incompatible with the control node. For instance, an x86 control node will not manage Arm nodes.

In addition to these requirements, Sira calls some common Linux utilities. Your systems will need to provide either these same tools or the drop-in replacements of your choice:

- GNU CoreUtils (chmod, chown, cp, mkdir, mktemp, mv, rm, users, whoami)
- OpenSSH client (control node)
- OpenSSH server (managed nodes)
- Sudo

This list might expand with future versions of Sira.

## Project status

Sira is a personal project. This means that I invest time in Sira at my sole discretion. I offer no commitments or assurances of any kind. Like most people, I am a human being with bills to pay and a life outside of software.

If you have a professional interest in Sira and you want to hire me to develop and maintain it, please feel free to get in touch! Of course, the license also permits you to fork it and develop it privately, if you prefer.

If community interest develops around Sira, I am open to the possibility of evolving the project into a collaborative, community effort.

**Sira is experimental!** The core of Sira is fully functional and well-documented. You can write manifest and task files, with or without cryptographic signatures, and they will execute fully and correctly. Outside of the core, some basic features, such as the UI, are not complete. Other planned features, such as the auto-update system, are not yet implemented on any level whatsoever. Additionally, breaking changes may occur at any time, as Sira has not yet reached its 0.1 release.

## License

Licensed under either the "Apache License (Version 2.0)" or the "MIT License" at your option. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
