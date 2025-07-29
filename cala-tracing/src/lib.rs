#![cfg_attr(feature = "fail-on-warnings", deny(warnings))]
#![cfg_attr(feature = "fail-on-warnings", deny(clippy::all))]

use opentelemetry::{trace::TracerProvider, KeyValue};
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, SdkProvidedResourceDetector},
    trace::Config,
    Resource,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_NAMESPACE};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{filter::EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub use tracing::*;

use std::time::Duration;

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
    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .with_trace_config(Config::default().with_resource(telemetry_resource(&config)))
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;
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
    Resource::from_detectors(
        Duration::from_secs(3),
        vec![
            Box::new(EnvResourceDetector::new()),
            Box::new(SdkProvidedResourceDetector),
        ],
    )
    .merge(&Resource::new(vec![
        KeyValue::new(SERVICE_NAME, config.service_name.clone()),
        KeyValue::new(SERVICE_NAMESPACE, "lava"),
    ]))
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
