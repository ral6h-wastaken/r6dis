use crate::{command::Command, resp::RespType};

#[derive(Debug)]
pub struct Redis {
    capacity: usize,
}

impl Redis {
    pub fn new() -> Self {
        Self { capacity: 0 }
    }

    pub fn handle_command(&mut self, cmd: Command) -> Result<RespType, String> {
        todo!()
    }
}
