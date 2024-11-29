use std::str::FromStr;

use axum::{
    extract::State, http::HeaderMap, response::{IntoResponse, Response}, routing::get, Router
};
use opentelemetry::{
    global,
    trace::{get_active_span, FutureExt, Span, Status, TraceContextExt, Tracer},
    Context, KeyValue,
};
use reqwest::{header, StatusCode};
use serde::Deserialize;
use tracing::{info, warn, Level};
use tracing_subscriber::{filter::Targets, layer::SubscriberExt, util::SubscriberInitExt};


#[derive(Clone)]
struct ServerState {
    client: reqwest::Client,
}

#[tokio::main]
async fn main() {
    let filter = Targets::from_str(std::env::var("RUST_LOG").as_deref().unwrap_or("info"))
        .expect("RUST_LOG should be a valid tracing filter");
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .json()
        .finish()
        .with(filter)
        .init();

    let _guard = sentry::init((
        std::env::var("SENTRY_DSN").expect("$SENTRY_DSN must be set"),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));

    let (_honeyguard, _tracer) = opentelemetry_honeycomb::new_pipeline(
        std::env::var("HONEYCOMB_API_KEY").expect("$HONEYCOMB_API_KEY should be set"),
        "catscii".into(),
    )
    .install()
    .unwrap();

    let state = ServerState {
        client: Default::default(),
    };

    let quit_sig = async {
        _ = tokio::signal::ctrl_c().await;
        warn!("Initiating graceful shutdown");
    };

    let app = Router::new()
        .route("/", get(root_get))
        .route("/panic", get(|| async { panic!("This is a test panic") }))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    info!("Listening on {}", 8080);
    axum::serve(listener, app).with_graceful_shutdown(quit_sig).await.unwrap();
}

async fn root_get(headers: HeaderMap, State(state): State<ServerState>) -> Response {
    let tracer = global::tracer("");
    let mut span = tracer.start("root_get");
    span.set_attribute(KeyValue::new(
        "user_agent",
        headers
            .get(header::USER_AGENT)
            .map(|h| h.to_str().unwrap_or_default().to_owned())
            .unwrap_or_default(),
    ));

    root_get_inner(state)
        .with_context(Context::current_with_span(span))
        .await
}

async fn root_get_inner(state: ServerState) -> Response {
    let tracer = global::tracer("");

    match get_cat_ascii_art(&state.client)
        .with_context(Context::current_with_span(
            tracer.start("get_cat_ascii_art"),
        ))
        .await
    {
        Ok(art) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            art,
        )
            .into_response(),
        Err(e) => {
            get_active_span(|span| {
                span.set_status(Status::Error {
                    description: format!("{e}").into(),
                })
            });
            (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response()
        }
    }
}

async fn get_cat_ascii_art(client: &reqwest::Client) -> color_eyre::Result<String> {
    let tracer = global::tracer("");

    // let image_url = get_cat_image_url(client)
    //     .with_context(Context::current_with_span(
    //         tracer.start("get_cat_image_url"),
    //     ))
    //     .await?;

    let image_bytes = download_file(client, "https://cataas.com/cat")
        .with_context(Context::current_with_span(tracer.start("download_file")))
        .await?;

    let image = tracer.in_span("image::load_from_memory", |cx| {
        let img = image::load_from_memory(&image_bytes)?;
        cx.span()
            .set_attribute(KeyValue::new("width", img.width() as i64));
        cx.span()
            .set_attribute(KeyValue::new("height", img.height() as i64));
        Ok::<_, color_eyre::eyre::Report>(img)
    })?;

    let ascii_art = tracer.in_span("artem::convert", |_cx| {
        artem::convert(
            image,
            &artem::config::ConfigBuilder::new()
                .target(artem::config::TargetType::HtmlFile)
                .build(),
        )
    });

    Ok(ascii_art)
}

async fn get_cat_image_url(client: &reqwest::Client) -> color_eyre::Result<String> {
    #[derive(Deserialize)]
    struct CatImage {
        url: String,
    }

    let api_url = "https://api.thecatapi.com/v1/images/search";

    let image = client
        .get(api_url)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<CatImage>>()
        .await?
        .pop()
        .ok_or_else(|| color_eyre::eyre::eyre!("The Cat API returned no images"))?;

    Ok(image.url)
}

async fn download_file(client: &reqwest::Client, url: &str) -> color_eyre::Result<Vec<u8>> {
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}
