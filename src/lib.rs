pub mod accounting;
mod api;
mod api_stream;
pub mod attestation;
pub mod auth;
mod bounded_json;
pub mod catalog;
mod failures;
#[cfg(test)]
mod process_alloc_tests;
// Shared admission and detached preflight, before API handoff.
mod generation;
// Reservation ownership is independent of HTTP delivery.
#[allow(dead_code)]
mod generation_owner;
pub mod health;
pub mod inference;
pub mod lifecycle;
pub mod server;
// Bounded delivery owners retain the shared heavy admission.
#[allow(dead_code)]
mod stream_owner;
pub mod telemetry;
pub mod tor_endorsement;
#[cfg(feature = "tor")]
pub mod tor_ingress;
pub mod tor_runtime;

pub const SERVICE_NAME: &str = "possums";
