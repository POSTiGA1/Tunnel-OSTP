pub mod frame;
pub mod padding;

pub use frame::{FrameHeader, FrameKind, FramedPacket};
pub use padding::{AdaptivePadder, PaddingStrategy, TrafficProfile};
