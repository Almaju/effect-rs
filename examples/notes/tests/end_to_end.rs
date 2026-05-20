//! End-to-end test: spin up a real HTTP server backed by in-memory
//! sqlite, exercise every endpoint via reqwest, assert on the responses.

use std::time::Duration;

use effect_example_notes::api::build_router;
use effect_example_notes::repo::open_db;
use effect_http_server::serve;
use tokio::net::TcpListener;

async fn spawn() -> std::net::SocketAddr {
    let db = open_db(":memory:").await.expect("init in-memory db");
    let router = build_router(db);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = serve(listener, router).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

#[tokio::test]
async fn create_then_list_then_get_roundtrip() {
    let addr = spawn().await;
    let client = reqwest::Client::new();

    // Create a note.
    let create_resp = client
        .post(format!("http://{addr}/notes/create"))
        .body(r#"{"title":"first","body":"hello"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let body = create_resp.text().await.unwrap();
    assert!(body.contains("\"id\""));

    // Extract the id (transparent newtype encodes as the inner integer).
    let id: i64 = body
        .split("\"id\":")
        .nth(1)
        .and_then(|s| s.trim_end_matches('}').trim().parse().ok())
        .expect("parse id from create response");

    // List should now contain it.
    let list_resp = client
        .post(format!("http://{addr}/notes/list"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(list_resp.status(), 200);
    let list_body = list_resp.text().await.unwrap();
    eprintln!("[debug] create body: {body}");
    eprintln!("[debug] list body: {list_body}");
    eprintln!("[debug] parsed id: {id}");
    assert!(list_body.contains("\"first\""));
    assert!(list_body.contains("\"hello\""));

    // Get the specific note.
    let get_resp = client
        .post(format!("http://{addr}/notes/get"))
        .body(format!(r#"{{"id":{id}}}"#))
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status(), 200);
    let got_body = get_resp.text().await.unwrap();
    assert!(
        got_body.contains("\"first\""),
        "expected 'first' in get response, got: {got_body}"
    );
}

#[tokio::test]
async fn create_with_invalid_json_returns_400() {
    let addr = spawn().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/notes/create"))
        .body("not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_with_missing_field_returns_400() {
    let addr = spawn().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/notes/create"))
        .body(r#"{"title":"just title"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(body.contains("body"), "expected schema error mentioning 'body', got: {body}");
}

#[tokio::test]
async fn get_unknown_id_returns_null_note() {
    let addr = spawn().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/notes/get"))
        .body(r#"{"id":99999}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    // Option-None encodes as JSON null at the `note` field.
    assert!(body.contains("\"note\":null") || body.contains("\"note\": null"),
        "expected note:null, got: {body}");
}

#[tokio::test]
async fn unknown_path_returns_404() {
    let addr = spawn().await;
    let resp = reqwest::get(format!("http://{addr}/whatever")).await.unwrap();
    assert_eq!(resp.status(), 404);
}
