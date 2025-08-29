#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

use opentelemetry::{trace::TracerProvider as _, KeyValue};
use opentelemetry_sdk::Resource;
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
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(telemetry_resource(&config))
        .build();
    let telemetry =
        tracing_opentelemetry::layer().with_tracer(provider.tracer(config.service_name));

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
        tracing::Span::current().set_parent(ctx)
    }
}

#[cfg(feature = "grpc")]
pub mod grpc {
    use opentelemetry::propagation::{Extractor, TextMapPropagator};
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    pub fn extract_tracing<T>(request: &tonic::Request<T>) {
        let propagator = TraceContextPropagator::new();
        let parent_cx = propagator.extract(&RequestContextExtractor(request));
        tracing::Span::current().set_parent(parent_cx)
    }

    struct RequestContextExtractor<'a, T>(&'a tonic::Request<T>);

    impl<T> Extractor for RequestContextExtractor<'_, T> {
        fn get(&self, key: &str) -> Option<&str> {
            self.0.metadata().get(key).and_then(|s| s.to_str().ok())
        }

        fn keys(&self) -> Vec<&str> {
            self.0
                .metadata()
                .keys()
                .filter_map(|k| {
                    if let tonic::metadata::KeyRef::Ascii(key) = k {
                        Some(key.as_str())
                    } else {
                        None
                    }
                })
                .collect()
        }
    }
}
