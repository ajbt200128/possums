pub mod accounting;
mod api;
mod api_stream;
pub mod attestation;
pub mod auth;
mod bounded_json;
pub mod catalog;
#[cfg(test)]
mod process_alloc_tests;
// Shared admission and detached preflight, before renderer handoff.
mod generation;
// Reservation ownership is independent of HTTP delivery.
#[allow(dead_code)]
mod generation_owner;
pub mod inference;
pub mod render;
// Bounded delivery owners retain the shared heavy admission.
#[allow(dead_code)]
mod stream_owner;
// Internal accepted-request composition.
mod streaming_chat;
pub mod telemetry;
pub mod web;

pub const SERVICE_NAME: &str = "possums";
