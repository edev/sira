# Sira: Simple Remote Administration

Sira ("SIGH-rah", but pronounce it however you please) is a tool for managing small collections of Linux computers (including virtual machines).

By focusing on small, simple deployments, Sira can favor ergonomics, readability, and obvious correctness. By only supporting Linux, Sira can integrate beautifully and natively into Linux workflows.

## Example: basic features

```yaml
# basic/manifest.yaml
---
name: Welcome to Sira!
hosts:
  - zen3-linux
include:
  - task.yaml
```

```yaml
# basic/task.yaml
---
name: Let's start with the hasics...
actions:
  - command:
      - apt-get install -y zsh
      - chsh --shell /bin/zsh me
  - upload:
      from:  files/.zshrc.base
      to:    /home/me/.zshrc
      user: me
      group: me
  - line_in_file:
      path: /home/me/.zshrc
      line: eval "$(ssh-agent -s)" >/dev/null
  - script:
      name: Install oh-my-zsh
      user: me
      contents: |
        #!/bin/bash

        cd
        if [[ -e ~/.oh-my-zsh ]]; then
          exit
        fi
        sh -c "$(curl -fsSL https://raw.github.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" "" --unattended --keep-zshrc
```

```bash
$ eval $(ssh-agent -s)
$ ssh-add ~/.ssh/sira
$ sira basic/manifest.yaml
```

## Example: scripting

```yaml
# Example showing Sira incorporating scripts and scripts incorporating Sira
```

## Example: variables

```yaml
# Example installing packages via apt or apt-get
```

## Example: disallow password-based SSH connections

```yaml
# manifests/base.yaml
---
name: debian-base
hosts:
  - zen4
include:
  - tasks/base.yaml

# tasks/base.yaml
---
name: Disallow password-based SSH connections
actions:
  - command:
      - cp $config_file $copy
  - line_in_file:
      path: $config_file
      line: PasswordAuthentication no
      pattern: "#PasswordAuthentication "
  # For extra peace of mind, verify PasswordAuthentication isn't enabled elsewhere.
  - script:
      name: Verify that password authentication is not enabled
      contents: |
        #!/bin/bash

        grep -q "^ *PasswordAuthentication yes" $config_file
        if [[ $? == 0 ]]; then
          >&2 echo "Found 'PasswordAuthentication yes'. Please update your task file."
          exit 1
        fi
  - script:
      name: Restart ssh.service if config changed
      contents: |
        #!/bin/bash

        diff $config_file $copy
        if [[ $? != 0 ]]; then
          systemctl restart ssh.service
        fi
  - command:
      - rm $copy
vars:
  config_file: /etc/ssh/sshd_config
  copy: /root/.sshd_config.tmp
```

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

## Operating system support

Sira is only tested on Linux with 64-bit x86 CPUs. It might work on other Unix-like systems such as MacOS and BSD, and it might work on other architectures like 64-bit Arm, but these are untested. It absolutely **will not** work on Windows.

Since Sira is written in Rust and compiled to binary, it absolutely **will not** manage systems that are binary-incompatible with the control node. For instance, an x86 control node will not manage Arm nodes.

## Project status

Sira is a personal project. This means that I invest time in Sira at my sole discretion. I offer no commitments or assurances of any kind. Like most people, I am a human being with bills to pay and a life outside of software.

If you have a professional interest in Sira and you want to hire me to develop and maintain it, please feel free to get in touch! Of course, the license also permits you to fork it and develop it privately, if you prefer.

If community interest develops around Sira, I am open to the possibility of evolving the project into a collaborative, community effort.

**Sira is experimental!** The core of Sira is fully functional and well-documented. You can write manifest and task files, with or without cryptographic signatures, and they will execute fully and correctly. Outside of the core, some basic features, such as the UI, are not complete. Other planned features, such as the auto-update system, are not yet implemented on any level whatsoever. Additionally, breaking changes may occur at any time, as Sira has not yet reached its 0.1 release.

## License

Licensed under either the "Apache License (Version 2.0)" or the "MIT License" at your option. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
