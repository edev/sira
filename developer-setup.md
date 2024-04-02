# Developer setup

Below are possibly incomplete notes on setting up a machine to develop Sira.

## The absolute basics

You'll need to clone your fork of the repository, of course.

You'll need to install Rust and Cargo, e.g. via rustup.

## Project-specific notes

After cloning the repository, you will probably find that running the tests (e.g. `cargo test`) fails with an OpenSSH error stating that permissions on the [test keys](resources/etc/sira/keys) are too open. This is expected. Change the keys' permissions to `0600`, and this issue should be resolved. See the [README](resources/etc/sira/keys/README.md) in that directory for more information.
