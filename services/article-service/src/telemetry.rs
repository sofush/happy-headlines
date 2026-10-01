use std::{
    sync::OnceLock,
    time::Duration,
};

use axum::{
    extract::MatchedPath,
    http::{
        HeaderMap,
        HeaderName,
        Request,
        Response,
    },
};
use opentelemetry::{
    KeyValue,
    global,
    propagation::Extractor,
    trace::{
        Status,
        TracerProvider as _,
    },
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};
use tower_http::trace::{
    MakeSpan,
    OnResponse,
};
use tracing_subscriber::{
    EnvFilter,
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

use tracing_opentelemetry::OpenTelemetrySpanExt;

static PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

pub fn init(instance: &str) -> anyhow::Result<()> {
    let service_name = "article-service";
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_timeout(Duration::from_secs(5))
        .build()?;

    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attributes([
            KeyValue::new("service.instance.id", instance.to_string()),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ])
        .build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    global::set_tracer_provider(provider.clone());
    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("info,article_service=debug,tower_http=info")
        }))
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(provider.tracer(service_name)),
        )
        .try_init()?;

    let _ = PROVIDER.set(provider);

    Ok(())
}

pub fn shutdown() {
    let Some(provider) = PROVIDER.get() else {
        return;
    };

    if let Err(e) = provider.force_flush() {
        tracing::error!("could not flush pending spans: {e}");
    }

    if let Err(e) = provider.shutdown() {
        tracing::error!("could not shut down the tracer provider: {e}");
    }
}

#[derive(Clone, Copy, Default)]
pub struct HttpMakeSpan;

impl<B> MakeSpan<B> for HttpMakeSpan {
    fn make_span(&mut self, request: &Request<B>) -> tracing::Span {
        let route = request
            .extensions()
            .get::<MatchedPath>()
            .map(|p| p.as_str().to_owned());

        let span = tracing::info_span!(
            "http.request",
            "otel.kind" = "server",
            "otel.name" = route.as_deref().unwrap_or("http.request"),
            "http.request.method" = %request.method(),
            "url.path" = %request.uri().path(),
            "http.route" = tracing::field::Empty,
        );

        if let Some(route) = route {
            span.record("http.route", route);
        }

        let parent = global::get_text_map_propagator(|propagator| {
            propagator.extract(&HeaderExtractor(request.headers()))
        });

        let _ = span.set_parent(parent);

        span
    }
}

#[derive(Clone, Copy, Default)]
pub struct HttpOnResponse;

impl<B> OnResponse<B> for HttpOnResponse {
    fn on_response(
        self,
        response: &Response<B>,
        latency: Duration,
        span: &tracing::Span,
    ) {
        let status = response.status();

        if status.is_server_error() {
            span.set_status(Status::error(status.as_u16().to_string()));
        }

        tracing::debug!(
            parent: span,
            status = status.as_u16(),
            latency_ms = latency.as_millis() as u64,
            "request completed"
        );
    }
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}
