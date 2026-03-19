//! OpenTelemetry OTLP export for named `provide --name` processes.

use crate::cli::cli::{Cli, Commands};
use crate::config::{ConfigDir, OtlpTelemetryConfig, OtlpTransport};
use opentelemetry::global;
use opentelemetry::Key;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{LogExporter, Protocol, SpanExporter, WithExportConfig, WithHttpConfig, WithTonicConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource::SERVICE_VERSION;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue, MetadataMap};

/// Errors while building the OTLP tracing/logging pipeline.
#[derive(Debug, Error)]
pub enum OtlpInitError {
    #[error(transparent)]
    Exporter(#[from] opentelemetry_otlp::ExporterBuildError),

    #[error(transparent)]
    Config(#[from] crate::config::Error),

    #[error("invalid OTLP/gRPC metadata header name: {0}")]
    InvalidHeaderName(String),

    #[error("invalid OTLP/gRPC metadata header value for key {0}")]
    InvalidHeaderValue(String),
}

/// Owns OpenTelemetry SDK providers for shutdown after the CLI run finishes.
#[derive(Debug)]
pub struct OtelHandles {
    tracer_provider: SdkTracerProvider,
    logger_provider: SdkLoggerProvider,
}

impl OtelHandles {
    /// When the command is `provide --name` and `[telemetry.otlp]` has `enabled = true`, builds
    /// OTLP exporters and providers. Otherwise returns `Ok(None)`.
    pub fn try_from_cli(cli: &Cli) -> Result<Option<Self>, OtlpInitError> {
        let Some(Commands::Provide(cmd)) = cli.command.as_ref() else {
            return Ok(None);
        };
        let Some(name) = cmd.name.as_deref() else {
            return Ok(None);
        };
        let cd = ConfigDir::new(
            cmd.config_dir
                .clone()
                .unwrap_or_else(crate::config::default_config_dir),
        );
        let file = cd.read(name)?;
        let Some(otlp) = file.telemetry.otlp else {
            return Ok(None);
        };
        if !otlp.enabled {
            return Ok(None);
        }
        Self::build_for_provider(name, &otlp)
    }

    fn build_for_provider(provider_name: &str, otlp: &OtlpTelemetryConfig) -> Result<Option<Self>, OtlpInitError> {
        let protocol = otlp.protocol.unwrap_or(OtlpTransport::HttpProtobuf);

        let resource = build_resource(provider_name, otlp)?;

        let span_exporter = build_span_exporter(otlp, protocol)?;
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_batch_exporter(span_exporter)
            .build();

        let log_exporter = build_log_exporter(otlp, protocol)?;
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(log_exporter)
            .build();

        global::set_tracer_provider(tracer_provider.clone());

        Ok(Some(Self {
            tracer_provider,
            logger_provider,
        }))
    }

    pub fn tracer_provider(&self) -> &SdkTracerProvider {
        &self.tracer_provider
    }

    pub fn logger_provider(&self) -> &SdkLoggerProvider {
        &self.logger_provider
    }

    /// Flushes and shuts down exporters (call after the subscriber is done recording).
    pub fn shutdown(self) {
        let _ = self.logger_provider.shutdown();
        let _ = self.tracer_provider.shutdown();
    }
}

fn build_resource(provider_name: &str, otlp: &OtlpTelemetryConfig) -> Result<Resource, OtlpInitError> {
    let mut builder = Resource::builder()
        .with_service_name("anymount-provider")
        .with_attribute(KeyValue::new(
            Key::new("anymount.provider.name"),
            provider_name.to_owned(),
        ))
        .with_attribute(KeyValue::new(
            Key::from_static_str("service.namespace"),
            "anymount",
        ))
        .with_attribute(KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")));

    if let Some(extra) = &otlp.resource_attributes {
        for (k, v) in extra {
            builder = builder.with_attribute(KeyValue::new(Key::new(k.clone()), v.clone()));
        }
    }

    Ok(builder.build())
}

fn metadata_from_headers(headers: &HashMap<String, String>) -> Result<MetadataMap, OtlpInitError> {
    let mut map = MetadataMap::new();
    for (k, v) in headers {
        let key = k
            .parse::<AsciiMetadataKey>()
            .map_err(|_| OtlpInitError::InvalidHeaderName(k.clone()))?;
        let value = AsciiMetadataValue::try_from(v.as_str())
            .map_err(|_| OtlpInitError::InvalidHeaderValue(k.clone()))?;
        map.insert(key, value);
    }
    Ok(map)
}

fn build_span_exporter(
    otlp: &OtlpTelemetryConfig,
    protocol: OtlpTransport,
) -> Result<SpanExporter, OtlpInitError> {
    match protocol {
        OtlpTransport::HttpProtobuf => {
            let mut builder = SpanExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_timeout(Duration::from_secs(10));
            if let Some(endpoint) = &otlp.endpoint {
                builder = builder.with_endpoint(endpoint.clone());
            }
            if let Some(headers) = &otlp.headers {
                builder = builder.with_headers(headers.clone());
            }
            Ok(builder.build()?)
        }
        OtlpTransport::Grpc => {
            let mut builder = SpanExporter::builder()
                .with_tonic()
                .with_timeout(Duration::from_secs(10));
            if let Some(endpoint) = &otlp.endpoint {
                builder = builder.with_endpoint(endpoint.clone());
            }
            if let Some(headers) = &otlp.headers {
                builder = builder.with_metadata(metadata_from_headers(headers)?);
            }
            Ok(builder.build()?)
        }
    }
}

fn build_log_exporter(
    otlp: &OtlpTelemetryConfig,
    protocol: OtlpTransport,
) -> Result<LogExporter, OtlpInitError> {
    match protocol {
        OtlpTransport::HttpProtobuf => {
            let mut builder = LogExporter::builder()
                .with_http()
                .with_protocol(Protocol::HttpBinary)
                .with_timeout(Duration::from_secs(10));
            if let Some(endpoint) = &otlp.endpoint {
                builder = builder.with_endpoint(endpoint.clone());
            }
            if let Some(headers) = &otlp.headers {
                builder = builder.with_headers(headers.clone());
            }
            Ok(builder.build()?)
        }
        OtlpTransport::Grpc => {
            let mut builder = LogExporter::builder()
                .with_tonic()
                .with_timeout(Duration::from_secs(10));
            if let Some(endpoint) = &otlp.endpoint {
                builder = builder.with_endpoint(endpoint.clone());
            }
            if let Some(headers) = &otlp.headers {
                builder = builder.with_metadata(metadata_from_headers(headers)?);
            }
            Ok(builder.build()?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_from_headers_accepts_authorization() {
        let mut h = HashMap::new();
        h.insert(
            "authorization".to_owned(),
            "Bearer test".to_owned(),
        );
        let m = metadata_from_headers(&h).expect("valid metadata");
        assert_eq!(
            m.get("authorization").expect("authorization present").to_str().expect("ascii"),
            "Bearer test"
        );
    }
}
