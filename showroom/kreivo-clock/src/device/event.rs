//! Lock-free event channel between cores.

/// Events pushed from the chain task (core 0) to the UI (core 1).
pub enum UiEvent {
    Live(bool),
    Block(u32),
    Collators([u32; 6]),
    Status(Status),
    /// Show config URL on the display (e.g., "http://10.49.209.176/").
    ConfigUrl(heapless::String<32>),
}

#[derive(Clone, Copy)]
pub enum Status {
    Dim(&'static str),
    Good(&'static str),
    Error(&'static str),
}
