#![allow(unused_imports, unused_variables)]
use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer, Responder, get, middleware, put, web, web::Data,
};
pub use n8n_rustful_operator::{self, State, telemetry};
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[get("/metrics")]
async fn metrics(c: Data<State>, _req: HttpRequest) -> impl Responder {
    let metrics = c.metrics();
    HttpResponse::Ok()
        .content_type("application/openmetrics-text; version=1.0.0; charset=utf-8")
        .body(metrics)
}

#[get("/health")]
async fn health(_: HttpRequest) -> impl Responder {
    HttpResponse::Ok().json("healthy")
}

#[get("/")]
async fn index(c: Data<State>, _req: HttpRequest) -> impl Responder {
    let d = c.diagnostics().await;
    HttpResponse::Ok().json(&d)
}

#[derive(Deserialize, Serialize)]
struct LogLevelBody {
    filter: String,
}

#[put("/log-level")]
async fn log_level(
    handle: Data<telemetry::LogFilterHandle>,
    body: web::Json<LogLevelBody>,
) -> impl Responder {
    match EnvFilter::try_new(&body.filter) {
        Ok(new_filter) => {
            handle.reload(new_filter).unwrap();
            HttpResponse::Ok().json(body.into_inner())
        }
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let reload_handle = telemetry::init().await;

    let state = State::default();
    let controller = n8n_rustful_operator::run(state.clone());

    let server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(state.clone()))
            .app_data(Data::new(reload_handle.clone()))
            .wrap(middleware::Logger::default().exclude("/health"))
            .service(index)
            .service(health)
            .service(metrics)
            .service(log_level)
    })
    .bind("0.0.0.0:8080")?
    .shutdown_timeout(5);

    tokio::join!(controller, server.run()).1?;
    Ok(())
}
