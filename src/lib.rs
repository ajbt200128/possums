pub mod accounting;
mod api;
mod api_stream;
pub mod attestation;
pub mod auth;
mod bounded_json;
pub mod catalog;
pub mod free;
#[cfg(test)]
mod process_alloc_tests;
// Shared admission and detached preflight, before API handoff.
mod generation;
// Reservation ownership is independent of HTTP delivery.
#[allow(dead_code)]
mod generation_owner;
pub mod inference;
pub mod server;
// Bounded delivery owners retain the shared heavy admission.
#[allow(dead_code)]
mod stream_owner;
pub mod telemetry;

pub const SERVICE_NAME: &str = "possums";
