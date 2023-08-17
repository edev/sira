# Sira: Simple Remote Administration

Sira ("SIGH-rah", but pronounce it however you please) is a tool for managing small collections of computers (including virtual machines) running Unix-like operating systems.

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

I test and use Sira on Debian Linux using amd64 CPUs. I believe it should work on BSD and perhaps other Unix-like OSes, but I have not personally tested them. It absolutely **will not** manage systems that are binary-incompatible with the host. To the best of my knowledge, it also will not work on Windows.

For the most part, the code itself is platform-agnostic. If you have a specific use case in mind and wish to contribute to expanding Sira's operating system support, you are welcome to reach out. The following are the known impediments to cross-platform Sira:

1. Sira uses the [openssh](https://crates.io/crates/openssh) crate, which saves me quite a bit of reading and possibly a lot of work. It works well for my own use case. However, its documentation explicitly warns that it is only compatible with Unix-like operating systems.

1. Sira is written in pure Rust, which has many benefits. It also transmits a client to each host it manages, and that client is written in Rust and compiled to binary. I'm sure it is possible to compile an array of client binaries for different operating systems and transfer different clients to different operating systems, but I have not developed such a system. This work will need to be done before Sira can manage computers running varied operating systems.

## License

Licensed under either the "Apache License (Version 2.0)" or the "MIT License" at your option. See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
