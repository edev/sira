use crossbeam::channel;
use sira::core::plan::Plan;
use sira::executor::{ChannelPair, Executor};
use sira::ui;
use std::thread;

fn main() {
    // The code here is, at this current stage, simply a mockup meant for smoke testing of the
    // initial type design. The idea is to verify that there aren't any obvious issues with things
    // like passing references or awkward API designs.

    let plan = Plan::new();

    // let (ui_state, exec_ui_state) = ui::State::new();

    // let (sender, receiver) = channel::unbounded();
    // let exec_logger = ChannelPair { sender, receiver };
    // let (sender, receiver) = channel::unbounded();
    // let exec_network = ChannelPair { sender, receiver };
    // let executor = Executor::new(exec_ui_state, exec_logger, exec_network);

    // let _exec_handle = thread::spawn(|| executor.run());

    // let idle_state = match ui_state {
    //     ui::State::Idle(idle_state) => idle_state,
    //     x => panic!("Expected idle state, but received: {:?}", x),
    // };
    // let _running_state = idle_state.start(plan);
}
