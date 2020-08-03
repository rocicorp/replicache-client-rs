use futures::join;
use nanoserde::DeJson;
use replicache_client::wasm;
use wasm_bindgen_test::wasm_bindgen_test_configure;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[derive(DeJson)]
struct OpenTransactionResponse {
    #[nserde(rename = "transactionId")]
    transaction_id: u32,
}

#[wasm_bindgen_test]
async fn test_dag() {
    wasm::exercise_prolly().await;
}

async fn dispatch(db: &str, rpc: &str, data: &str) -> Result<String, String> {
    match wasm::dispatch(db.to_string(), rpc.to_string(), data.to_string()).await {
        Ok(v) => Ok(v),
        Err(v) => Err(v.as_string().unwrap()),
    }
}

#[wasm_bindgen_test]
async fn test_dispatch() {
    assert_eq!(dispatch("", "debug", "open_dbs").await.unwrap(), "[]");
    assert_eq!(
        dispatch("", "open", "").await.unwrap_err(),
        "db_name must be non-empty"
    );
    assert_eq!(dispatch("db", "open", "").await.unwrap(), "");
    assert_eq!(dispatch("", "debug", "open_dbs").await.unwrap(), "[\"db\"]");
    assert_eq!(dispatch("db2", "open", "").await.unwrap(), "");
    assert_eq!(
        dispatch("", "debug", "open_dbs").await.unwrap(),
        "[\"db\", \"db2\"]"
    );
    assert_eq!(dispatch("db", "close", "").await.unwrap(), "");
    assert_eq!(dispatch("db", "close", "").await.unwrap(), "");
    assert_eq!(
        dispatch("", "debug", "open_dbs").await.unwrap(),
        "[\"db2\"]"
    );
    assert_eq!(dispatch("db2", "close", "").await.unwrap(), "");
    assert_eq!(dispatch("", "debug", "open_dbs").await.unwrap(), "[]");
}

#[wasm_bindgen_test]
async fn test_dispatch_concurrency() {
    let window = web_sys::window().expect("should have a window in this context");
    let performance = window
        .performance()
        .expect("performance should be available");

    assert_eq!(dispatch("db", "open", "").await.unwrap(), "");
    let now_ms = performance.now();
    join!(
        async {
            dispatch("db", "get", "{\"key\": \"sleep100\"}")
                .await
                .unwrap();
        },
        async {
            dispatch("db", "get", "{\"key\": \"sleep100\"}")
                .await
                .unwrap();
        }
    );
    let elapsed_ms = performance.now() - now_ms;
    assert_eq!(dispatch("db", "close", "").await.unwrap(), "");
    assert_eq!(elapsed_ms >= 100., true);
    assert_eq!(elapsed_ms < 200., true);
}

#[wasm_bindgen_test]
async fn test_get_put() {
    assert_eq!(
        dispatch("db", "put", "{\"k\", \"v\"}").await.unwrap_err(),
        "\"db\" not open"
    );
    assert_eq!(dispatch("db", "open", "").await.unwrap(), "");

    // Check request parsing, both missing and unexpected fields.
    assert_eq!(
        dispatch("db", "put", "{}").await.unwrap_err(),
        "InvalidJson(Json Deserialize error: Key not found transaction_id, line:1 col:3)"
    );

    let transaction_resp: OpenTransactionResponse =
        match DeJson::deserialize_json(&dispatch("db", "openTransaction", "").await.unwrap()) {
            Ok(v) => v,
            Err(e) => panic!("Failed to parse openTransactionResponse: {}", e),
        };
    let transaction_id = transaction_resp.transaction_id;

    // With serde we can use #[serde(deny_unknown_fields)] to parse strictly,
    // but that's not available with nanoserde.
    assert_eq!(
        dispatch(
            "db",
            "get",
            &format!(
                "{{\"transactionId\": {}, \"key\": \"Hello\", \"value\": \"世界\"}}",
                transaction_id
            )
        )
        .await
        .unwrap(),
        "{\"has\":false}", // unwrap_err() == "Failed to parse request"
    );

    // Simple put then get test.
    // TODO(nate): Resolve how to pass non-UTF-8 sequences through the API.
    assert_eq!(
        dispatch(
            "db",
            "put",
            &format!(
                "{{\"transactionId\": {}, \"key\": \"Hello\", \"value\": \"世界\"}}",
                transaction_id
            )
        )
        .await
        .unwrap(),
        "{}"
    );
    assert_eq!(
        dispatch(
            "db",
            "get",
            &format!(
                "{{\"transactionId\": {}, \"key\": \"Hello\"}}",
                transaction_id
            )
        )
        .await
        .unwrap(),
        "{\"value\":\"世界\",\"has\":true}"
    );

    // NOCOMMIT: Commit.

    // Open new transaction, and verify write is persistent.
    let transaction_resp: OpenTransactionResponse =
        match DeJson::deserialize_json(&dispatch("db", "openTransaction", "").await.unwrap()) {
            Ok(v) => v,
            Err(e) => panic!("Failed to parse openTransactionResponse: {}", e),
        };
    let transaction_id = transaction_resp.transaction_id;
    assert_eq!(
        dispatch(
            "db",
            "get",
            &format!(
                "{{\"transactionId\": {}, \"key\": \"Hello\"}}",
                transaction_id
            )
        )
        .await
        .unwrap(),
        "{\"value\":\"世界\",\"has\":true}"
    );

    /*

    // Verify functioning of non-ASCII keys.
    assert_eq!(
        dispatch("db", "has", "{\"key\": \"你好\"}").await.unwrap(),
        "{\"has\":false}"
    );
    assert_eq!(
        dispatch("db", "get", "{\"key\": \"你好\"}").await.unwrap(),
        "{\"has\":false}"
    );
    assert_eq!(
        dispatch("db", "put", "{\"key\": \"你好\", \"value\": \"world\"}")
            .await
            .unwrap(),
        "{}"
    );
    assert_eq!(
        dispatch("db", "has", "{\"key\": \"你好\"}").await.unwrap(),
        "{\"has\":true}"
    );
    assert_eq!(
        dispatch("db", "get", "{\"key\": \"你好\"}").await.unwrap(),
        "{\"value\":\"world\",\"has\":true}"
    );

    assert_eq!(dispatch("db", "close", "").await.unwrap(), "");
    */
}
