use super::*;
use serde_json::{json, Value};
use tauri::{
    ipc::{CallbackFn, InvokeBody},
    test::{get_ipc_response, mock_builder, INVOKE_KEY},
    webview::InvokeRequest,
    Manager,
};

struct CancelledDialogs;
impl dialogs::Dialogs for CancelledDialogs {
    fn choose(&self, _: bool) -> EngineResult<Vec<std::path::PathBuf>> {
        Ok(vec![])
    }
}
fn app() -> tauri::App<tauri::test::MockRuntime> {
    // Use production context/ACL and command registration, with OS dialogs replaced
    // so tests never open a picker or access personal media.
    configure(
        mock_builder(),
        NativeState {
            session: Mutex::default(),
            dialogs: Arc::new(CancelledDialogs),
        },
    )
    .build(context())
    .unwrap()
}
fn window(
    app: &tauri::App<tauri::test::MockRuntime>,
    label: &str,
) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .unwrap()
}
fn invoke(
    w: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(
        w,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
    .map(|response| response.deserialize::<Value>().unwrap())
}
fn code(result: Result<Value, Value>, expected: &str) {
    let error = result.unwrap_err();
    assert_eq!(error["code"], expected, "{error}");
    assert_eq!(error["retryable"], false);
    assert_eq!(error.as_object().unwrap().len(), 3);
}
#[test]
fn startup_and_all_commands_use_v1_ipc() {
    let app = app();
    let w = window(&app, "main");
    assert_eq!(invoke(&w, "get_api_version", json!({})), Ok(json!(1)));
    let formats = invoke(&w, "list_formats", json!({})).unwrap();
    assert_eq!(
        formats,
        serde_json::to_value(loconv_engine::formats::descriptors()).unwrap()
    );
    assert_eq!(formats.as_array().unwrap().len(), 7);
    assert!(formats
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["available"] == false));
    // Actual event plugin subscription must be permitted before list_jobs.
    let subscription = invoke(
        &w,
        "plugin:event|listen",
        json!({
            "event": "loconv://job-changed", "target": {"kind":"Any"}, "handler": 1
        }),
    )
    .unwrap();
    assert!(subscription.is_number());
    assert_eq!(invoke(&w, "list_jobs", json!({})), Ok(json!([])));
    assert_eq!(invoke(&w, "select_inputs", json!({})), Ok(json!([])));
    assert_eq!(invoke(&w, "select_destination", json!({})), Ok(Value::Null));
    assert_eq!(
        invoke(&w, "import_dropped_inputs", json!({"paths":[]})),
        Ok(json!([]))
    );
    code(
        invoke(
            &w,
            "import_dropped_inputs",
            json!({"paths":["/synthetic/invented.png"]}),
        ),
        "tool_unavailable",
    );
    for (format, name, reason) in [
        ("mp4_h264_aac", "synthetic.mp4", "tool_unavailable"),
        ("png", "synthetic.png", "unsupported_format"),
    ] {
        code(
            invoke(
                &w,
                "enqueue_job",
                json!({"request": {
                    "mediaId":"invented-input", "destinationId":"invented-destination",
                    "formatId":format, "outputName":name
                }}),
            ),
            reason,
        );
    }
    assert_eq!(invoke(&w, "list_jobs", json!({})), Ok(json!([])));
    code(
        invoke(&w, "cancel_job", json!({"jobId":"invented-job"})),
        "job_not_found",
    );
    invoke(
        &w,
        "plugin:event|unlisten",
        json!({"event":"loconv://job-changed", "eventId":subscription}),
    )
    .unwrap();
}
#[test]
fn malformed_arguments_are_safe_errors() {
    let app = app();
    let w = window(&app, "main");
    for cmd in [
        "get_api_version",
        "list_formats",
        "list_jobs",
        "select_inputs",
        "select_destination",
    ] {
        code(
            invoke(&w, cmd, json!({"extra":"sensitive-placeholder"})),
            "invalid_request",
        );
    }
    for body in [
        json!({}),
        json!({"paths":"bad"}),
        json!({"paths":[],"extra":true}),
        json!({"paths":["https://invalid.example/synthetic"]}),
        json!({"paths":["file:/synthetic.png"]}),
        json!({"paths":["relative.png"]}),
    ] {
        code(invoke(&w, "import_dropped_inputs", body), "invalid_request");
    }
    for body in [
        json!({}),
        json!({"jobId":""}),
        json!({"jobId":"x","extra":true}),
    ] {
        code(invoke(&w, "cancel_job", body), "invalid_request");
    }
    for body in [
        json!({}),
        json!({"request":{"mediaId":"x","destinationId":"y","outputName":"a.png","formatId":"png","extra":true}}),
        json!({"request":{"mediaId":"x","destinationId":"y","outputName":"../escape.png","formatId":"png"}}),
    ] {
        code(invoke(&w, "enqueue_job", body), "invalid_request");
    }
    code(
        invoke(
            &w,
            "import_dropped_inputs",
            json!({"paths":vec!["/synthetic.png";101]}),
        ),
        "resource_limit",
    );
}
#[test]
fn cancellation_publishes_one_revision_and_is_idempotent() {
    let app = app();
    let w = window(&app, "main");
    let state = app.state::<NativeState>();
    let queued = state
        .session
        .lock()
        .unwrap()
        .jobs
        .register_queued(ConversionRequest {
            media_id: "synthetic".into(),
            destination_id: "synthetic".into(),
            format_id: FormatId::Png,
            output_name: "synthetic.png".into(),
        })
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    use tauri::Listener;
    let received = events.clone();
    w.listen("loconv://job-changed", move |event| {
        received
            .lock()
            .unwrap()
            .push(serde_json::from_str::<JobEvent>(event.payload()).unwrap());
    });
    let cancelled = invoke(&w, "cancel_job", json!({"jobId":queued.job_id})).unwrap();
    assert_eq!(cancelled["state"], "cancelled");
    assert_eq!(cancelled["revision"], 2);
    assert_eq!(cancelled["error"], Value::Null);
    assert_eq!(cancelled["progress"], Value::Null);
    assert_eq!(
        invoke(&w, "cancel_job", json!({"jobId":queued.job_id})).unwrap(),
        cancelled
    );
    assert_eq!(events.lock().unwrap().len(), 1);
    assert_eq!(events.lock().unwrap()[0].api_version, API_VERSION);
    assert_eq!(invoke(&w, "list_jobs", json!({})), Ok(json!([cancelled])));
    let new_session = session::Session::default();
    assert!(new_session.jobs.list().is_empty());
}
#[test]
fn capability_denies_other_windows() {
    let app = app();
    let w = window(&app, "untrusted");
    assert!(invoke(&w, "list_jobs", json!({})).is_err());
    assert!(invoke(
        &w,
        "plugin:event|listen",
        json!({"event":"loconv://job-changed","target":{"kind":"Any"},"handler":1})
    )
    .is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn selected_destination_is_an_opaque_bounded_session_capability() {
    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir(&self.0).unwrap();
        }
    }
    let dir =
        TempDir(std::env::temp_dir().join(format!("loconv-synthetic-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&dir.0).unwrap();
    struct SelectedDirectory(std::path::PathBuf);
    impl dialogs::Dialogs for SelectedDirectory {
        fn choose(&self, directory: bool) -> EngineResult<Vec<std::path::PathBuf>> {
            assert!(directory);
            Ok(vec![self.0.clone()])
        }
    }
    let app = configure(
        mock_builder(),
        NativeState {
            session: Mutex::default(),
            dialogs: Arc::new(SelectedDirectory(dir.0.clone())),
        },
    )
    .build(context())
    .unwrap();
    let w = window(&app, "main");
    let selected = invoke(&w, "select_destination", json!({})).unwrap();
    assert_eq!(selected.as_object().unwrap().len(), 2);
    assert!(uuid::Uuid::parse_str(selected["destinationId"].as_str().unwrap()).is_ok());
    assert!(!selected
        .to_string()
        .contains(dir.0.parent().unwrap().to_str().unwrap()));
    for _ in 1..100 {
        invoke(&w, "select_destination", json!({})).unwrap();
    }
    code(
        invoke(&w, "select_destination", json!({})),
        "resource_limit",
    );
    assert_eq!(invoke(&w, "list_jobs", json!({})), Ok(json!([])));
}

#[test]
fn selected_inputs_fail_explicitly_without_reading_any_media() {
    struct SelectedInputs;
    impl dialogs::Dialogs for SelectedInputs {
        fn choose(&self, directory: bool) -> EngineResult<Vec<std::path::PathBuf>> {
            assert!(!directory);
            Ok(vec!["/synthetic/nonexistent.png".into()])
        }
    }
    let app = configure(
        mock_builder(),
        NativeState {
            session: Mutex::default(),
            dialogs: Arc::new(SelectedInputs),
        },
    )
    .build(context())
    .unwrap();
    let w = window(&app, "main");
    code(invoke(&w, "select_inputs", json!({})), "tool_unavailable");
    assert_eq!(invoke(&w, "list_jobs", json!({})), Ok(json!([])));
}
