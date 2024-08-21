use std::time::Duration;

use actix_web::{web, App, HttpServer};
use actix_web_opentelemetry::RequestTracing;
use opentelemetry::trace::{Span, Status, Tracer};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::resource::{
    EnvResourceDetector, ResourceDetector, SdkProvidedResourceDetector, TelemetryResourceDetector,
};
use opentelemetry_sdk::trace::Config;
use opentelemetry_sdk::{runtime, Resource};
use serde::{Deserialize, Serialize};
use tonic::metadata::MetadataMap;

#[derive(Deserialize)]
struct FibonacciRequest {
    n: i64,
}

#[derive(Serialize)]
struct FibonacciResult {
    #[serde(skip_serializing_if = "is_zero")]
    #[serde(default)]
    n: i64,
    #[serde(skip_serializing_if = "is_zero")]
    #[serde(default)]
    result: i64,
    #[serde(skip_serializing_if = "is_empty")]
    #[serde(default)]
    message: String,
}

fn is_zero(num: &i64) -> bool {
    *num == 0
}

fn is_empty(str: &String) -> bool {
    str.is_empty()
}

async fn fibonacci(req: web::Query<FibonacciRequest>) -> web::Json<FibonacciResult> {
    let result_or_error = compute_fibonacci(req.n);

    match result_or_error {
        Ok(result) => web::Json(FibonacciResult {
            n: req.n,
            result: result,
            message: String::new(),
        }),
        Err(error) => web::Json(FibonacciResult {
            n: 0,
            result: 0,
            message: format!("{}", error),
        }),
    }
}

fn compute_fibonacci(n: i64) -> Result<i64, Box<dyn std::error::Error>> {
    let tracer = global::tracer("fibonacci_server");
    let meter = global::meter("fibonacci_server_metric");

    let mut span = tracer.span_builder("fibonacci").start(&tracer);

    let counter = meter.u64_counter("fibo_counter").init();

    counter.add(1, &[KeyValue::new("id", "1234")]);

    span.set_attribute(KeyValue::new("fibonacci.n", n));

    if n < 1 || n > 90 {
        let err_msg = "n must be between 1 and 90";
        span.set_status(Status::error(err_msg));
        // span.record_error(err);
        return Err(Box::from(err_msg));
    }

    let mut result: i64 = 1;
    if n > 2 {
        let mut a: i64 = 0;
        let mut b: i64 = 1;

        let mut i: i64 = 1;
        while i < n {
            result = a + b;
            a = b;
            b = result;
            i = i + 1;
        }
    }
    span.set_attribute(KeyValue::new("fibonacci.result", result));
    Ok(result)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let sdk_provided_resource = SdkProvidedResourceDetector.detect(Duration::from_secs(0));
    let env_resource = EnvResourceDetector::new().detect(Duration::from_secs(0));
    let telemetry_resource = TelemetryResourceDetector.detect(Duration::from_secs(0));
    let resource = sdk_provided_resource
        .merge(&env_resource)
        .merge(&telemetry_resource);

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_tls_config(tonic::transport::ClientTlsConfig::new().with_native_roots()),
        )
        .with_trace_config(Config::default().with_resource(resource.clone()))
        .install_batch(runtime::Tokio)
        .expect("failed to initialize the trace pipeline");

    let metric_provider = opentelemetry_otlp::new_pipeline()
        .metrics(opentelemetry_sdk::runtime::Tokio) // Use the Tokio runtime
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_tls_config(tonic::transport::ClientTlsConfig::new().with_native_roots()), // Configure TLS
        )
        .with_resource(resource) // Reuse the resource from span setup
        .build()
        .expect("failed to initialize the metrics pipeline");

    global::set_meter_provider(metric_provider);
    global::set_tracer_provider(tracer_provider);

    HttpServer::new(|| {
        App::new()
            .wrap(RequestTracing::new())
            .route("/fibonacci", web::get().to(fibonacci))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await?;

    global::shutdown_tracer_provider();

    Ok({})
}
