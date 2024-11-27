use std::str::FromStr;

use axum::{
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use reqwest::{header, StatusCode};
use serde::Deserialize;
use tracing::{info, Level};
use tracing_subscriber::{filter::Targets, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let filter = Targets::from_str(std::env::var("RUST_LOG").as_deref().unwrap_or("info")).expect("RUST_LOG should be a valid tracing filter");
    tracing_subscriber::fmt().with_max_level(Level::TRACE)
        .json()
        .finish()
        .with(filter)
        .init();

    let app = Router::new().route("/", get(root_get));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    info!("Listening on {}", 8080);
    axum::serve(listener, app).await.unwrap();
}

async fn root_get() -> Response {
    match get_cat_ascii_art().await {
        Ok(art) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            art,
        )
            .into_response(),
        Err(e) => {
            println!("Something went wrong: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into_response()
        }
    }
}

async fn get_cat_ascii_art() -> color_eyre::Result<String> {
    #[derive(Deserialize)]
    struct CatImage {
        url: String,
    }

    let api_url = "https://api.thecatapi.com/v1/images/search";
    let client = reqwest::Client::default();

    let image = client
        .get(api_url)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<CatImage>>()
        .await?
        .pop()
        .ok_or_else(|| color_eyre::eyre::eyre!("The Cat API returned no images"))?;

    let image_bytes = client
        .get(image.url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let image = image::load_from_memory(&image_bytes)?;
    let ascii_art = artem::convert(
        image,
        &artem::config::ConfigBuilder::new()
            .target(artem::config::TargetType::HtmlFile)
            .build(),
    );

    Ok(ascii_art)
}
