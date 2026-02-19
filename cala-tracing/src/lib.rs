#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

use opentelemetry::{global, trace::TracerProvider, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    trace::{Sampler, SdkTracerProvider},
    Resource,
};
use opentelemetry_semantic_conventions::resource::SERVICE_NAMESPACE;
use serde::{Deserialize, Serialize};
use tracing_subscriber::{filter::EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub use tracing::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingConfig {
    service_name: String,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            service_name: "cala-dev".to_string(),
        }
    }
}

pub fn init_tracer(config: TracingConfig) -> anyhow::Result<()> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_resource(telemetry_resource(&config))
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::AlwaysOn)
        .build();

    global::set_tracer_provider(provider.clone());
    let tracer = provider.tracer("cala-tracer");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    let fmt_layer = fmt::layer().json();
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info,otel::tracing=trace,sqlx=warn"))
        .unwrap();
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(telemetry)
        .init();

    Ok(())
}

fn telemetry_resource(config: &TracingConfig) -> Resource {
    Resource::builder()
        .with_service_name(config.service_name.clone())
        .with_attributes([KeyValue::new(SERVICE_NAMESPACE, "cala")])
        .build()
}

pub fn insert_error_fields(level: tracing::Level, error: impl std::fmt::Display) {
    Span::current().record("error", tracing::field::display("true"));
    Span::current().record("error.level", tracing::field::display(level));
    Span::current().record("error.message", tracing::field::display(error));
}

#[cfg(feature = "http")]
pub mod http {
    pub fn extract_tracing(headers: &axum_extra::headers::HeaderMap) {
        use opentelemetry::propagation::text_map_propagator::TextMapPropagator;
        use opentelemetry_http::HeaderExtractor;
        use opentelemetry_sdk::propagation::TraceContextPropagator;
        use tracing_opentelemetry::OpenTelemetrySpanExt;
        let extractor = HeaderExtractor(headers);
        let propagator = TraceContextPropagator::new();
        let ctx = propagator.extract(&extractor);
        let _ = tracing::Span::current().set_parent(ctx);
    }
}
