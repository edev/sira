/// The central component of Sira's controller-side software.
///
/// Provides the communication hub among the user interface, logger, network, and any plans
/// being run. Coordinates the execution of plans on managed nodes.
#[allow(dead_code)]
pub struct Executor<U, L, N>
where
    U: UserInterface,
    L: Logger,
    N: Network,
{
    ui: U,
    logger: L,
    network: N,
}

/// The public API to a user interface.
pub trait UserInterface {}

/// The public API to a logging mechanism.
pub trait Logger {}

/// The public API to a system for connecting to computers it will manage, typically via SSH.
pub trait Network {}
