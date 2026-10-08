//! Supported OTel conversion and bounded owned transport; no ambient SDK builder.
pub(super) mod handoff;
mod materialize;
pub(super) mod transport;

const ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

async fn encode_and_send(
    client: &transport::Client,
    metrics: &opentelemetry_sdk::metrics::data::ResourceMetrics,
) -> Result<(), transport::Failure> {
    use opentelemetry_http::HttpClient;
    use prost::Message;
    // Exactly the public conversion used by OTel 0.29's HTTP exporter. Avoid
    // its environment-reading builder entirely: no inherited headers, resources,
    // protocols, proxies, diagnostics or global environment mutation.
    let message =
        opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest::from(
            metrics,
        );
    if message.encoded_len() > transport::MAX_OUTBOUND {
        return Err(transport::Failure::Unavailable);
    }
    let bytes = hyper::body::Bytes::from(message.encode_to_vec());
    drop(message);
    let request = http::Request::post(client.endpoint())
        .body(bytes)
        .map_err(|_| transport::Failure::Configuration)?;
    client
        .send_bytes(request)
        .await
        .map_err(|_| transport::Failure::Transport)?;
    Ok(())
}

// The historical 1000-line fixture/stock-exporter harness is not a runtime module.
#[cfg(test)]
include!("export/qualification.rs");
