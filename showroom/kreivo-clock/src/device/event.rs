//! Lock-free event channel between cores.

/// Events pushed from the chain task (core 0) to the UI (core 1).
pub enum UiEvent {
    Wifi(bool),
    Live(bool),
    Block(u32),
    Collators([u32; 6]),
    Status(Status),
}

#[derive(Clone, Copy)]
pub enum Status {
    Dim(&'static str),
    Good(&'static str),
    Error(&'static str),
}
