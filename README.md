# Sira: Simple Remote Administration

Sira (officially pronounced "SIGH-rah", but pronounce it however you wish) is a tool for managing small collections of Linux computers (including virtual machines). By focusing on small, simple deployments, Sira can favor ergonomics, readability, and obvious correctness over more advanced features. By only supporting Linux, Sira can integrate beautifully and natively into Linux workflows. For simple, Linux-focused networks, Sira aims to make automating system administration across the whole network as simple as writing the commands in an SSH session or script. Sira tries to be transparent: it's designed never to be another, quirky abstraction layer you have to think about.

Power users and experienced system administrators will (hopefully) feel right at home using Sira. Basically, if you're comfortable using the terminal to administer your systems, the learning curve should be minimal.

On the other hand, if you're more of a novice Linux user and you're reading about Sira, welcome! Please don't hesitate to give Sira a try. You won't find a one-liner to set up Samba, NFS, or Kerberos, but you'll certainly learn a ton by configuring them yourself. Plus, Sira itself is very simple, so you won't need to spend two weeks learning how to use it.

## Getting started

To get a feel for Sira, keep reading below. Once you're ready to try it for yourself, work through the [installation guide](/installation.md) and return here once you're done.

Sira is split into a control node application called `sira` and a client application called `sira-client`. You pass `sira` a list of files containing instructions, and it connects to nodes via SSH and invokes `sira-client` to execute those instructions. There is no always-on server, and you do not need to open any extra ports. In fact, in the default configuration, the account that runs `sira-client` is far better secured than a typical administrator's account! (For details, see [security.md](/security.md).)

### Installing `sira-client` on a managed node

After working through the [installation guide](/installation.md), preparing a managed node to run Sira is as simple as:

```bash
# Add the Sira user to the managed node. Also, store the managed node's SSH fingerprint.
ssh -t <managed-node-admin>@<managed-node> sudo useradd [options] <sira-user>

# Install Sira on the managed node. On first run, sets up the control node, too.
sira-install <sira-user> <managed-node-admin>@<managed-node>
```

That's it!

### Actions

Sira supports a deliberately simple, minimal set of instructions, which Sira calls **actions**:

```yaml
# Run one or more commands on managed nodes (as root).
#
# Note that these processes are created directly and are not interpreted by a shell, so shell
# features like ~ and | will not work. You can always invoke a shell, if you need one.
- command:
    - apt-get install -y qemu-system-x86 snapd
    - snap install core
    - sudo -u alice bash -c "mkdir -p ~/.ssh"

# For more complex logic, you can run shell scripts in arbitrary languages and as arbitrary users.
- script:
    name: Install or update Rust
    user: alice

    # Note the "|" after contents. This enables block scalar syntax, which tells Sira to treat the
    # shebang as part of the script and not as a YAML comment.
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
- If Sira is ever uploaded to [crates.io](https://crates.io), you will be able to view it at [docs.rs](https://docs.rs). Click on the `core` module and then the `Action` enum. (This currently is not the case.)
- Clone this repository, and then run `cargo doc --open` from the repository's directory. Click on the `core` module and then the `Action` enum.
- Browse to [/src/core/action.rs](src/core/action.rs) and read the documentation in source form.

### Tasks

Sira organizes lists of actions as **tasks**. Each task is a YAML document, meaning that it starts with `---`. You can write multiple tasks in a single file, if you prefer, or stick to one task per file. A task groups actions and optionally defines variables for those actions (explained later):

```yaml
---
name: Install system packages
actions:
  - command:
      - apt-get install -y $apt_packages
      - snap install core
      - flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
      - flatpak install --noninteractive flathub $flathub_bundles
vars:
  # We'll make these much more readable a little later in this guide!
  apt_packages: build-essential flatpak git qemu-system-x86 snapd
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

Sira groups task files into **manifests** that associate task files with managed nodes (i.e. hosts). Just like tasks, you can write multiple manifests in a manifest file or stick to one per file. Note that you cannot place manifests and tasks in the same file. Example:

```yaml
---
name: debian-base
hosts:
  - alice-laptop
include:
  # Paths here are relative to the manifest file.
  - tasks/debian-base.yaml
vars:
  # A "configure Grub" task might insert this value into /etc/default/grub.
  grub_timeout: 10
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

If a managed node is unreachable, Sira will ignore it and continue processing other nodes. At the end of the run, `sira` will exit with a `0` exit code signaling success.

If an action fails on any managed node, that node aborts, and the other nodes continue processing. Once the run is complete, `sira` will exit with a non-zero exit code.

### Updating Sira and `sira-client`

Sira is designed around the assumption that you will incorporate it into whatever command-line tooling works for you. It doesn't provide an explicit auto-update facility, but updating Sira is very simple nonetheless.

First, update Sira on the control node. For instance, if you cloned Sira into `~/sira`:

```bash
cd ~/sira
git pull
cargo install --path .
```

Then, write a task **at the very beginning of your first-run task file** that installs the latest `sira-client` binary on a managed node (remember to change `<control-user>` to your Sira control user name):

```yaml
---
name: Update sira-client
actions:
  - upload:
      from: /home/<control-user>/.cargo/bin/sira-client
      to: /opt/sira/bin/sira-client
      permissions: 700
```

That's it!

Sira might add an option to use GitHub releases as a chain of trust for signed binaries in the future; if so, Sira might include an auto-update mechanism by default. At this time, however, the above is the recommended way to keep Sira up-to-date.

### Advanced feature: Variables

As the examples above demonstrated, manifests and tasks can define variables for use in actions by using the `vars` key. When `sira` is about to run an action on a managed node, it compiles a copy of the action to send to `sira-client` as YAML. As part of this process, it substitutes variables into all fields except Booleans, e.g. `indent` for `line_in_file` and `overwrite` for `upload`. (This is due to a minor technical limitation; if there's demand, applying variables to Boolean fields can be implemented.)

The process is intentionally simple, both so that you don't have to think about complex mechanics and so that Sira can remain agnostic of shell languages, etc. Right before sending an action to a managed node:

1. Sira compiles a dictionary mapping variable names to values.
2. For each variable `v`, Sira searches all fields of the action (except Booleans) for occurrences of `$v` or `${v}` and replaces that text with the variable's value. This is implemented as a single regular expression match-and-replace operation; it is not recursive.

Details are below, but as long as you keep things simple, all you need to remember are the two steps above.

Manifest variables override task variables. This allows you to use a variable in a task, provide a default value, and optionally override it when you include the task in a manifest.

Sira replaces variables in the order in which they were defined. However, it is a *really bad idea* to depend on this behavior, e.g. to try to create recursive variable substitutions. It will work, but your files will almost certainly become inscrutable and nearly impossible to maintain!

Variables are not substituted in manifests or in other fields of tasks (e.g. `name`). They are only applied to actions and only in the manner stated above. There is no other "magic."

For maximum flexibility, there is no error detection when substituting variables.

For even more detail, read the documentation for `sira::core::action::HostAction::compile()`.

### Advanced feature: harness the full power of YAML

The choice to use YAML for Sira instead of a more ubiquitous language like JSON is intentional: YAML is a very powerful language with features that can augment your manifests and tasks. (JSON is a subset of YAML, so you can technically write JSON instead, if you are sufficiently determined. The docs do not cover this use case.) The `script` action actually depends on an advanced feature of YAML called block scalar syntax, as noted in the examples above.

Using a closely related feature, folded scalar syntax, we can clean up the package lists from the examples above:

```yaml
---
name: Install system packages
actions:
  - command:
      - apt-get install -y $apt_packages
      - snap install core
      - flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
      - flatpak install --noninteractive flathub $flathub_bundles
vars:
  apt_packages: >-
    build-essential
    flatpak
    git
    qemu-system-x86
    snapd
  flathub_bundles: >-
    com.discordapp.Discord
    com.vscodium.codium
```

If you find a creative way to harness the power of YAML to improve your manifest and task files, feel free to open an issue to discuss adding it here!

### Advanced feature: use Sira in shell scripts

`sira` works like a standard terminal application. It takes all its inputs as command-line arguments. It keeps running in the foreground until all instructions are complete. It aborts if an action returns a non-zero exit status, and it exits with a non-zero status in the event of a failure. Thus, `sira` integrates cleanly with shell scripts. You can script complex sequences of actions easily, e.g.:

```bash
#!/bin/bash

set -e

sira configure-server.yaml
scp server:files-to-distribute files
sira distribute-files.yaml
```

### Advanced feature: Cryptographically sign manifests, tasks, and actions

Sira supports signing manifest and task files as well as actions sent to `sira-client`. If these keys are installed, `sira` will refuse to execute unsigned or improperly signed manifest and task files, and `sira-client` will refuse to execute unsigned or improperly signed actions. See [security.md](/security.md) for details on how this works and [installation.md](/installation.md) for instructions on setting this up. For most users, `sira-install` handles this automatically.

If you are using cryptographic signing, you can sign your manifest and task files after changes using OpenSSH's `ssh-keygen`:

```bash
ssh-keygen -Y sign -n sira -f <path-to-key> <file-name> ...
```

### Advanced technique: use task files as plugins

Sira deliberately lacks support for plugins, extensions, and so on. However, you can achieve similar effects (code reuse and abstraction) by writing task files that incorporate well-documented manifest variables.

For example, if you have a standard set of Git configuration options that you set across many users, you might write a file `tasks/configure_git.yaml` to encapsulate these actions:

```yaml
# Configures Git for a single user.
#
# Expected manifest variables:
# user: the Linux user name for whom Git will be configured.
# name: the value to set as Git's user.name.
# email: The value to set as Git's user.email.

---
name: Install Git
actions:
  - command:
      - apt install -y git
      - sudo -u "$user" git config --global user.name "$name"
      - sudo -u "$user" git config --global user.email "$email"
```

You can then use this plugin-like (or function-like) task file in your manifest files, setting the appropriate variables each time you include it:

```yaml
---
name: Configure Git for alice
hosts:
  - alice-laptop
include:
  - tasks/configure_git.yaml
vars:
  user: alice
  name: Alice Realperson
  email: alice@example.com

---
name: Configure Git for alice-work
hosts:
  - alice-laptop
include:
  - tasks/configure_git.yaml
vars:
  user: alice-work
  name: Alice Realperson
  email: alice@example.com
```

### Advanced technique: assert success (or handle failure)

Since Sira stops running actions on a given host when an action on that host exits with a failure exit code, it's trivial to insert checks or even error-handling logic into your tasks. For instance:

```yaml
name: Update /etc/apt/sources.list
actions:
  - command:
      # Guard against using old release names, URIs, and so on.
      - grep -q     "deb http://deb.debian.org/debian/ $release main" $sources
      - grep -q "deb-src http://deb.debian.org/debian/ $release main" $sources
      # ...

      # Save a copy to detect changes later and run apt update if needed.
      - cp $sources $copy
  # Primary
  - line_in_file:
      path: $sources
      line: deb http://deb.debian.org/debian/ $release main contrib non-free non-free-firmware
      pattern: deb http://deb.debian.org/debian/ $release main
  # Primary (source)
  - line_in_file:
      path: $sources
      line: deb-src http://deb.debian.org/debian/ $release main contrib non-free non-free-firmware
      pattern: deb-src http://deb.debian.org/debian/ $release main
  # ...
  - script:
      name: Run apt update if sources changed
      contents: |
        #!/bin/bash

        diff $sources $copy
        if [[ $? != 0 ]]; then
          apt-get update
        fi
  - command:
      - rm $copy
vars:
  sources: /etc/apt/sources.list
  copy: /root/.sources.list.tmp
  release: bookworm
```

This task checks to make sure that Apt source lines look as expected before modifying sources.list, updates the file as needed, and runs `apt-get update` if and only if the file changes. This code works on Debian 12 (bookworm) and will deliberately and safely fail on other versions in order to tell you that it's time to update your `release` variable.

## Why not use Ansible, Chef, Puppet, Salt, etc.?

If these tools work well for you, great! Keep using them!

These tools are very sophisticated and very complex. They are designed for use cases as large as enterprise deployments, and their feature sets reflect this. For simple deployments such as homelabs and personal networks, this complexity is often unnecessary. Sira is designed with small-scale, simpler deployments in mind. As a result, Sira makes different choices and different trade-offs.

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

Sira is a personal project. This means that I invest time in Sira at my sole discretion. I offer no commitments or assurances of any kind. Like most people, I am a human being with bills to pay and a life outside of software. You are quite welcome to use it and even to provide feedback! I might or might not respond, though.

If you have a professional interest in Sira and you want to hire me to develop and maintain it, please feel free to get in touch! Of course, the license also permits you to fork it and develop it privately, if you prefer.

### Why isn't Sira published on crates.io?

Let me set aside my professional voice for a moment to explain my personal feelings and motivations. In my view, publishing a crate is an invitation for the world to try, use, and provide feedback on a project, and it comes with a responsibility to support those users as well. I'm not really looking to throw a house party on my GitHub, nor am I prepared to take responsibility for being the solo maintainer of a (free!) project that competes with IBM, Puppet Labs, and more. I wrote Sira for my own, personal use, and I'm showing it off here because I think it's cool. If others think it's cool and a community develops around Sira, great! My feelings might change. However, that will require other, talented developers coalescing around a positive, nourishing, collaborative, community effort in which I personally want to participate. Otherwise, Sira might simply live here forever (or might one day disappear, possibly without warning).

### Sira is experimental!

The core of Sira is fully functional and well-documented. You can write manifest and task files, with or without cryptographic signatures, and they will execute fully and correctly. Outside of the core, some basic features, such as the UI, are not complete. Other planned features, such as the auto-update system, are not yet implemented on any level whatsoever. Additionally, breaking changes may occur at any time.

**Major to-do items include:**

- Develop a self-updater
- Develop `Action::RegexInFile`
- Replace `serde_yaml` now that it's deprecated
- Replace `sira` and `sira-client` binaries with more fully featured versions (e.g. implementations that use [clap](https://crates.io/crates/clap) to accept arguments and support a reasonable set of them)

## Why so many dependencies?

Sira itself only has a few, carefully chosen dependencies, all of which serve important purposes. Sira also takes care to enable only the minimal set of required features of said dependencies. However, some of these have extensive dependency graphs of their own. As a result, Sira winds up with some absurd dependencies like Windows API support. Clearing these indirect dependencies is not practical for Sira at this time, but there are at least three likely ways this might change in the future.

First, [Rust 1.60](https://blog.rust-lang.org/2022/04/07/Rust-1.60.0.html) adds several features for better dependency management, like the ability for a crate's features to enable features on dependencies only if those dependencies are already enabled. Sira's dependency [Tokio](https://tokio.rs), for instance, supports Rust versions much lower than 1.60 at time of writing, which prevents Tokio from using these features. In time, the minimum supported Rust version (MSRV) will advance, and projects will be able to consider using such features.

Second, there might be room to work with libraries in Sira's dependency graph to add new feature flags or provide other mechanisms that will ultimately pare down Sira's dependency graph.

Third, Sira might simply remove some of these dependencies and implement the required features internally. Sira actually started out this way, and it might return to this in the future. This would also offer opportunities to improve the feedback from running processes over SSH, which is a known shortcoming of Sira's interface right now.

The primary reason all of this matters for Sira is supply chain security. As an extraordinarily security-sensitive application, it makes sense for Sira to be very careful about including third-party source code. On the other hand, sometimes it makes sense to rely on widely trusted and vetted public infrastructure. The long-term goal is to make careful and intentional choices between internal and external code in order to maximize software quality while minimizing risk.

All of this is to say that if you have ideas for paring down Sira's dependency graph in sensible ways, please feel free to get in touch!

## License

Licensed under either the "Apache License (Version 2.0)" or the "MIT License" at your option. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
