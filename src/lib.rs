pub mod accounting;
pub mod attestation;
pub mod auth;
mod bounded_json;
pub mod catalog;
// Staged ownership only; buffered /chat remains unchanged.
#[allow(dead_code)]
mod generation_owner;
pub mod inference;
pub mod render;
// Staged internal primitive; route/worker integration is a separate packet.
#[allow(dead_code)]
mod stream_owner;
pub mod telemetry;
pub mod web;

pub const SERVICE_NAME: &str = "possums";
