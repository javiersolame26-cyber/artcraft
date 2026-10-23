//! A client for a ComfyUI server running on the user's own machine (or LAN).
//!
//! ArtCraft uses it as a free, local generation provider: workflows are queued
//! on ComfyUI, polled through its history endpoint, and the outputs are
//! downloaded from its `/view` endpoint.

pub mod api;
pub mod client;
pub mod error;
pub mod workflows;

#[cfg(test)]
pub(crate) mod test_server;
