//! Protocol conformance: the JSON examples from the LocalSend v2 specification
//! (github.com/localsend/protocol) parse into our types and serialize back to the same JSON,
//! and the HTTP endpoints answer with the documented bodies and status codes.

mod common;

use std::collections::HashMap;

use common::*;
use panoply_share::http::{self, Conn};
use panoply_share::proto::{
    DeviceInfo, DeviceType, PrepareDownloadResponse, PrepareUploadRequest, PrepareUploadResponse,
    Scheme,
};
use serde_json::{Value, json};

// ---- the spec's examples, verbatim in structure --------------------------------------------

fn multicast_announcement() -> Value {
    json!({
        "alias": "Nice Orange", "version": "2.0", "deviceModel": "Samsung", "deviceType": "mobile",
        "fingerprint": "random string", "port": 53317, "protocol": "https", "download": true,
        "announce": true
    })
}

fn register_request() -> Value {
    json!({
        "alias": "Secret Banana", "version": "2.0", "deviceModel": "Windows",
        "deviceType": "desktop", "fingerprint": "random string", "port": 53317,
        "protocol": "https", "download": true
    })
}

fn register_response() -> Value {
    json!({
        "alias": "Nice Orange", "version": "2.0", "deviceModel": "Samsung", "deviceType": "mobile",
        "fingerprint": "random string", "download": true
    })
}

fn udp_fallback_response() -> Value {
    json!({
        "alias": "Secret Banana", "version": "2.0", "deviceModel": "Windows",
        "deviceType": "desktop", "fingerprint": "random string", "port": 53317,
        "protocol": "https", "download": true, "announce": false
    })
}

fn prepare_upload_request() -> Value {
    json!({
        "info": {
            "alias": "Nice Orange", "version": "2.0", "deviceModel": "Samsung",
            "deviceType": "mobile", "fingerprint": "random string", "port": 53317,
            "protocol": "https", "download": true
        },
        "files": {
            "someFileId": {
                "id": "someFileId", "fileName": "my image.png", "size": 324242,
                "fileType": "image/jpeg", "sha256": "a3f5...", "preview": "base64 preview data",
                "metadata": { "modified": "2021-01-01T12:34:56Z", "accessed": "2021-01-01T12:34:56Z" }
            },
            "anotherFileId": {
                "id": "anotherFileId", "fileName": "another image.jpg", "size": 1234,
                "fileType": "image/jpeg", "sha256": "9c1e...", "preview": "base64 preview data"
            }
        }
    })
}

fn prepare_upload_response() -> Value {
    json!({
        "sessionId": "mySessionId",
        "files": { "someFileId": "someFileToken", "someOtherFileId": "someOtherFileToken" }
    })
}

fn prepare_download_response() -> Value {
    json!({
        "info": {
            "alias": "Nice Orange", "version": "2.0", "deviceModel": "Samsung",
            "deviceType": "mobile", "fingerprint": "random string", "download": true
        },
        "sessionId": "mySessionId",
        "files": {
            "someFileId": {
                "id": "someFileId", "fileName": "my image.png", "size": 324242,
                "fileType": "image/jpeg", "sha256": "a3f5...", "preview": "base64 preview data"
            }
        }
    })
}

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(v: Value) -> T {
    let parsed: T = serde_json::from_value(v.clone()).expect("the spec example parses");
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        v,
        "serializes back to the same JSON"
    );
    parsed
}

#[test]
fn device_info_examples_round_trip() {
    let a: DeviceInfo = round_trip(multicast_announcement());
    assert_eq!(a.announce, Some(true));
    assert_eq!(a.kind(), DeviceType::Mobile);
    assert_eq!(a.scheme_or_default(), Scheme::Https);
    assert!(a.version_compatible());
    let _: DeviceInfo = round_trip(register_request());
    let _: DeviceInfo = round_trip(udp_fallback_response());
    let r: DeviceInfo = round_trip(register_response());
    // The response example leaves port and protocol out; the defaults apply.
    assert_eq!(r.port_or_default(), 53317);
    assert_eq!(r.scheme_or_default(), Scheme::Https);
}

#[test]
fn older_announcement_flag_is_understood() {
    let mut v = multicast_announcement();
    v.as_object_mut().unwrap().remove("announce");
    v["announcement"] = json!(true);
    let d: DeviceInfo = serde_json::from_value(v).unwrap();
    assert_eq!(d.announce, Some(true));
}

#[test]
fn transfer_examples_round_trip() {
    let req: PrepareUploadRequest = round_trip(prepare_upload_request());
    assert_eq!(req.files.len(), 2);
    assert_eq!(req.files["someFileId"].file_name, "my image.png");
    let _: PrepareUploadResponse = round_trip(prepare_upload_response());
    let dl: PrepareDownloadResponse = round_trip(prepare_download_response());
    assert_eq!(dl.session_id, "mySessionId");
}

#[test]
fn constants_match_the_spec() {
    assert_eq!(panoply_share::proto::DEFAULT_PORT, 53317);
    assert_eq!(
        panoply_share::proto::MULTICAST_GROUP.to_string(),
        "224.0.0.167"
    );
    assert_eq!(panoply_share::proto::API_PREFIX, "/api/localsend/v2");
}

// ---- the HTTP surface -----------------------------------------------------------------------

async fn raw(
    node: &panoply_share::Node,
    method: hyper::Method,
    path: &str,
    body: Vec<u8>,
) -> (u16, Vec<u8>) {
    let mut c: Conn = http::connect(
        node.local_addr(),
        Scheme::Https,
        None,
        std::time::Duration::from_secs(5),
    )
    .await
    .unwrap();
    let (code, bytes) = c.call(method, path, http::full(body)).await.unwrap();
    (code, bytes.to_vec())
}

fn post() -> hyper::Method {
    hyper::Method::POST
}

#[tokio::test]
async fn info_and_register_answer_with_device_info() {
    let node = sender("Probe").await;
    let (code, body) = raw(&node, hyper::Method::GET, "/api/localsend/v2/info", vec![]).await;
    assert_eq!(code, 200);
    let info: DeviceInfo = serde_json::from_slice(&body).unwrap();
    assert_eq!(info.alias, "Probe");
    assert_eq!(info.fingerprint, node.fingerprint());
    assert_eq!(info.fingerprint.len(), 64);
    assert!(info.version.starts_with("2."));

    let (code, body) = raw(
        &node,
        post(),
        "/api/localsend/v2/register",
        serde_json::to_vec(&register_request()).unwrap(),
    )
    .await;
    assert_eq!(code, 200);
    let me: DeviceInfo = serde_json::from_slice(&body).unwrap();
    assert_eq!(me.fingerprint, node.fingerprint());
    // The registered device is now known.
    assert_eq!(node.devices().len(), 1);
    assert_eq!(node.devices()[0].alias(), "Secret Banana");

    let (code, _) = raw(
        &node,
        post(),
        "/api/localsend/v2/register",
        b"{nope".to_vec(),
    )
    .await;
    assert_eq!(code, 400);
    let (code, _) = raw(
        &node,
        hyper::Method::GET,
        "/api/localsend/v2/nothing",
        vec![],
    )
    .await;
    assert_eq!(code, 404);
}

#[tokio::test]
async fn prepare_upload_and_upload_status_codes() {
    let dst = TempDir::new("dst");
    let rx = accepting_receiver("R", dst.path()).await;

    // 400: invalid body.
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        b"not json".to_vec(),
    )
    .await;
    assert_eq!(code, 400);
    // 400: a file id that differs from its key.
    let mut bad = prepare_upload_request();
    bad["files"]["someFileId"]["id"] = json!("other");
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&bad).unwrap(),
    )
    .await;
    assert_eq!(code, 400);

    // 200 with a session and a token per file, exactly the spec's example request.
    let (code, body) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&prepare_upload_request()).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
    let resp: PrepareUploadResponse = serde_json::from_slice(&body).unwrap();
    let tokens: HashMap<_, _> = resp.files.clone();
    assert_eq!(tokens.len(), 2);

    // 409: another session is running.
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&prepare_upload_request()).unwrap(),
    )
    .await;
    assert_eq!(code, 409);

    let sid = &resp.session_id;
    let up = |file: &str, token: &str| {
        format!("/api/localsend/v2/upload?sessionId={sid}&fileId={file}&token={token}")
    };
    // 403: wrong token, unknown session.
    let (code, _) = raw(&rx, post(), &up("anotherFileId", "wrong"), vec![1; 1234]).await;
    assert_eq!(code, 403);
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/upload?sessionId=nope&fileId=anotherFileId&token=x",
        vec![],
    )
    .await;
    assert_eq!(code, 403);
    // 400: fewer bytes than declared.
    let (code, _) = raw(
        &rx,
        post(),
        &up("anotherFileId", &tokens["anotherFileId"]),
        vec![1; 10],
    )
    .await;
    assert_eq!(code, 400);
    // 422: the sha256 in the example does not match the content.
    let (code, _) = raw(
        &rx,
        post(),
        &up("anotherFileId", &tokens["anotherFileId"]),
        vec![1; 1234],
    )
    .await;
    assert_eq!(code, 422);
    assert!(
        tree(dst.path()).is_empty(),
        "a rejected upload leaves nothing behind"
    );

    // Cancel frees the session: a new request is accepted again.
    let (code, _) = raw(
        &rx,
        post(),
        &format!("/api/localsend/v2/cancel?sessionId={sid}"),
        vec![],
    )
    .await;
    assert_eq!(code, 200);
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&prepare_upload_request()).unwrap(),
    )
    .await;
    assert_eq!(code, 200);
}

#[tokio::test]
async fn upload_accepts_the_declared_bytes_and_checks_the_hash() {
    let dst = TempDir::new("dst");
    let rx = accepting_receiver("R", dst.path()).await;
    let data = blob(5000, 9);
    let sha = {
        use sha2::{Digest, Sha256};
        panoply_share::proto::hex(&Sha256::digest(&data))
    };
    let mut req = prepare_upload_request();
    req["files"] = json!({
        "f1": { "id": "f1", "fileName": "ok.bin", "size": 5000, "fileType": "application/octet-stream", "sha256": sha }
    });
    let (code, body) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&req).unwrap(),
    )
    .await;
    assert_eq!(code, 200);
    let resp: PrepareUploadResponse = serde_json::from_slice(&body).unwrap();
    let path = format!(
        "/api/localsend/v2/upload?sessionId={}&fileId=f1&token={}",
        resp.session_id, resp.files["f1"]
    );
    let (code, _) = raw(&rx, post(), &path, data.clone()).await;
    assert_eq!(code, 200);
    assert_eq!(std::fs::read(dst.join("ok.bin")).unwrap(), data);
    // The session is complete: the same upload again finds no session.
    let (code, _) = raw(&rx, post(), &path, data).await;
    assert_eq!(code, 403);
}

#[tokio::test]
async fn pin_status_codes() {
    let dst = TempDir::new("dst");
    let mut cfg = loopback_config("R");
    let mut rc = panoply_share::ReceiveConfig::new(dst.path(), panoply_share::accept_all_handler());
    rc.pin = Some("123456".into());
    cfg.receive = Some(rc);
    let rx = panoply_share::Node::start(cfg).await.unwrap();
    let body = serde_json::to_vec(&prepare_upload_request()).unwrap();
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        body.clone(),
    )
    .await;
    assert_eq!(code, 401);
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload?pin=000000",
        body.clone(),
    )
    .await;
    assert_eq!(code, 401);
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload?pin=123456",
        body,
    )
    .await;
    assert_eq!(code, 200);
}

#[tokio::test]
async fn declined_and_text_status_codes() {
    let dst = TempDir::new("dst");
    let rx = receiver("R", dst.path(), panoply_share::decline_all_handler()).await;
    let (code, _) = raw(
        &rx,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&prepare_upload_request()).unwrap(),
    )
    .await;
    assert_eq!(code, 403);

    // A single text/plain file whose preview is its content is a message: 204, no upload.
    let ok = accepting_receiver("R2", dst.path()).await;
    let mut msg = prepare_upload_request();
    msg["files"] = json!({
        "m": { "id": "m", "fileName": "m.txt", "size": 5, "fileType": "text/plain", "preview": "hello" }
    });
    let (code, _) = raw(
        &ok,
        post(),
        "/api/localsend/v2/prepare-upload",
        serde_json::to_vec(&msg).unwrap(),
    )
    .await;
    assert_eq!(code, 204);
}
