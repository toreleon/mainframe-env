use axum::body::Body;
use axum::http::Request;
use mainframe_env_server::{ProductServer, ServerConfig, StoreProfile, TlsConfig};
use std::path::PathBuf;
use tower::ServiceExt;

#[tokio::main]
async fn main() {
    if let Err(problem) = run().await {
        eprintln!("certify: {problem}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let multiplier = std::env::args()
        .nth(1)
        .ok_or("usage: certify <1|2|long-run>")?;
    let (requests, label) = match multiplier.as_str() {
        "1" => (256usize, "1x"),
        "2" => (512usize, "2x"),
        "long-run" => (10_000usize, "long-run"),
        _ => return Err("usage: certify <1|2|long-run>".into()),
    };
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: PathBuf::from(format!(
            "{}-mainframe-env-certify-artifacts",
            std::env::temp_dir().display()
        )),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let server = ProductServer::memory(config).map_err(|problem| problem.to_string())?;
    let app = server.router();
    let mut tasks = Vec::with_capacity(requests);
    for _ in 0..requests {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .uri("/zosmf/info")
                    .body(Body::empty())
                    .expect("static request"),
            )
            .await
            .map(|response| response.status().as_u16())
        }));
    }
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for task in tasks {
        match task.await {
            Ok(Ok(200)) => succeeded += 1,
            _ => failed += 1,
        }
    }
    let metrics = server.metrics();
    let shutdown = server.graceful_shutdown().await;
    println!(
        "{{\"profile\":\"{label}\",\"offered\":{requests},\"succeeded\":{succeeded},\"failed\":{failed},\"active_after\":{},\"sessions_after\":{},\"shutdown_drained\":{shutdown}}}",
        metrics.active, metrics.sessions
    );
    if failed != 0 || succeeded != requests || metrics.active != 0 || !shutdown {
        return Err("load or leak gate failed".into());
    }
    Ok(())
}
