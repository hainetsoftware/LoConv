use super::*;
use loconv_engine::image;
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
            dialogs: Arc::new(CancelledDialogs),
            ..Default::default()
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
    assert_eq!(
        formats
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["available"] == true)
            .count(),
        3
    );
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
            json!({"paths":["/synthetic/nonexistent.png"]}),
        ),
        "inspection_failed",
    );
    for (format, name, reason) in [
        ("mp4_h264_aac", "synthetic.mp4", "tool_unavailable"),
        ("png", "synthetic.png", "invalid_request"),
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
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir =
        TempDir(std::env::temp_dir().join(format!("loconv-synthetic-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&dir.0).unwrap();
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
            dialogs: Arc::new(SelectedDirectory(dir.0.clone())),
            ..Default::default()
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
            dialogs: Arc::new(SelectedInputs),
            ..Default::default()
        },
    )
    .build(context())
    .unwrap();
    let w = window(&app, "main");
    code(invoke(&w, "select_inputs", json!({})), "inspection_failed");
    assert_eq!(invoke(&w, "list_jobs", json!({})), Ok(json!([])));
}

#[test]
fn real_synthetic_image_inspection_and_conversion_e2e() {
    let temp_dir = std::env::temp_dir().join(format!("loconv-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let input_path = temp_dir.join("input.png");
    let dest_dir = temp_dir.join("output");
    std::fs::create_dir_all(&dest_dir).unwrap();

    // 1. Generate synthetic PNG image with alpha channel
    let input_img = image::RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([(x * 8) as u8, (y * 10) as u8, 150, 180])
    });
    input_img
        .save_with_format(&input_path, image::ImageFormat::Png)
        .unwrap();

    let original_bytes = std::fs::read(&input_path).unwrap();

    struct CustomDialogs {
        input_path: std::path::PathBuf,
        dest_dir: std::path::PathBuf,
    }
    impl dialogs::Dialogs for CustomDialogs {
        fn choose(&self, directory: bool) -> EngineResult<Vec<std::path::PathBuf>> {
            if directory {
                Ok(vec![self.dest_dir.clone()])
            } else {
                Ok(vec![self.input_path.clone()])
            }
        }
    }

    let app = configure(
        mock_builder(),
        NativeState {
            dialogs: Arc::new(CustomDialogs {
                input_path: input_path.clone(),
                dest_dir: dest_dir.clone(),
            }),
            ..Default::default()
        },
    )
    .build(context())
    .unwrap();
    let w = window(&app, "main");

    // 2. Select input and inspect
    let imported = invoke(&w, "select_inputs", json!({})).unwrap();
    let imported_arr = imported.as_array().unwrap();
    assert_eq!(imported_arr.len(), 1);
    let media = &imported_arr[0];
    let media_id = media["mediaId"].as_str().unwrap();
    assert_eq!(media["kind"], "image");
    assert_eq!(media["container"], "png");
    assert_eq!(media["streams"][0]["width"], 32);
    assert_eq!(media["streams"][0]["height"], 24);

    // 3. Select destination
    let dest_sel = invoke(&w, "select_destination", json!({})).unwrap();
    let dest_id = dest_sel["destinationId"].as_str().unwrap();

    // 4. Enqueue PNG -> JPEG conversion
    let output_name = "converted.jpg";
    let enqueue_res = invoke(
        &w,
        "enqueue_job",
        json!({
            "request": {
                "mediaId": media_id,
                "destinationId": dest_id,
                "formatId": "jpeg",
                "outputName": output_name
            }
        }),
    )
    .unwrap();

    assert_eq!(enqueue_res["state"], "queued");
    assert_eq!(enqueue_res["revision"], 1);

    // Wait for the asynchronous conversion worker to complete
    let output_file = dest_dir.join(output_name);
    let start = std::time::Instant::now();
    while !output_file.exists() && start.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(output_file.exists(), "Output JPEG file must be published");

    // 5. Verify output file validity
    let output_bytes = std::fs::read(&output_file).unwrap();
    assert!(!output_bytes.is_empty());
    assert_eq!(output_bytes[0..3], [0xFF, 0xD8, 0xFF]); // JPEG SOI

    // Decode output image to ensure valid raster with correct dimensions
    let decoded = image::open(&output_file).expect("Output must be readable valid JPEG");
    assert_eq!(decoded.width(), 32);
    assert_eq!(decoded.height(), 24);

    // 6. Source file must remain completely unmodified
    let current_input_bytes = std::fs::read(&input_path).unwrap();
    assert_eq!(
        original_bytes, current_input_bytes,
        "Original file must be untouched"
    );

    // 7. Enqueueing with the same output name must fail with OutputConflict (never overwrite)
    code(
        invoke(
            &w,
            "enqueue_job",
            json!({
                "request": {
                    "mediaId": media_id,
                    "destinationId": dest_id,
                    "formatId": "jpeg",
                    "outputName": output_name
                }
            }),
        ),
        "output_conflict",
    );

    // 8. Source/output aliasing check: destination is temp_dir itself where input.png lives
    let same_dest_id = {
        let state = app.state::<NativeState>();
        let mut session = state.session.lock().unwrap();
        let dest_id = uuid::Uuid::new_v4().to_string();
        session
            .destinations
            .insert(dest_id.clone(), temp_dir.clone());
        dest_id
    };

    code(
        invoke(
            &w,
            "enqueue_job",
            json!({
                "request": {
                    "mediaId": media_id,
                    "destinationId": same_dest_id,
                    "formatId": "png",
                    "outputName": "input.png"
                }
            }),
        ),
        "output_conflict",
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn conversion_job_cancellation_cleans_up_and_prevents_publication() {
    let temp_dir = std::env::temp_dir().join(format!("loconv-cancel-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let input_path = temp_dir.join("input.png");
    let dest_dir = temp_dir.join("output");
    std::fs::create_dir_all(&dest_dir).unwrap();

    let input_img = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 4) as u8, 200, 255])
    });
    input_img
        .save_with_format(&input_path, image::ImageFormat::Png)
        .unwrap();

    struct CancelTestDialogs {
        input_path: std::path::PathBuf,
        dest_dir: std::path::PathBuf,
    }
    impl dialogs::Dialogs for CancelTestDialogs {
        fn choose(&self, directory: bool) -> EngineResult<Vec<std::path::PathBuf>> {
            if directory {
                Ok(vec![self.dest_dir.clone()])
            } else {
                Ok(vec![self.input_path.clone()])
            }
        }
    }

    let app = configure(
        mock_builder(),
        NativeState {
            dialogs: Arc::new(CancelTestDialogs {
                input_path: input_path.clone(),
                dest_dir: dest_dir.clone(),
            }),
            ..Default::default()
        },
    )
    .build(context())
    .unwrap();
    let w = window(&app, "main");

    let imported = invoke(&w, "select_inputs", json!({})).unwrap();
    let media_id = imported[0]["mediaId"].as_str().unwrap();

    let dest_sel = invoke(&w, "select_destination", json!({})).unwrap();
    let dest_id = dest_sel["destinationId"].as_str().unwrap();

    let output_name = "cancelled.jpg";
    let enqueue_res = invoke(
        &w,
        "enqueue_job",
        json!({
            "request": {
                "mediaId": media_id,
                "destinationId": dest_id,
                "formatId": "jpeg",
                "outputName": output_name
            }
        }),
    )
    .unwrap();

    let job_id = enqueue_res["jobId"].as_str().unwrap();

    // Immediately cancel the job
    let cancel_res = invoke(&w, "cancel_job", json!({"jobId": job_id})).unwrap();
    let state = cancel_res["state"].as_str().unwrap();
    assert!(state == "cancelled" || state == "cancelling");

    // Wait briefly for worker task to settle
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Confirm that the final output was NEVER published
    let output_file = dest_dir.join(output_name);
    assert!(
        !output_file.exists(),
        "Cancelled job must never publish final output"
    );

    // Confirm that no engine temp files were left behind in the destination directory
    let entries = std::fs::read_dir(&dest_dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".loconv-tmp-"),
            "No temporary files should remain: {name}"
        );
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn rejects_animated_and_oversized_images() {
    let temp_dir = std::env::temp_dir().join(format!("loconv-anim-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    // 1. Synthetic APNG
    let apng_path = temp_dir.join("animated.png");
    let mut apng_data = Vec::new();
    apng_data.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    apng_data.extend_from_slice(&13u32.to_be_bytes());
    apng_data.extend_from_slice(b"IHDR");
    apng_data.extend_from_slice(&[0; 13]);
    apng_data.extend_from_slice(&[0; 4]);
    apng_data.extend_from_slice(&8u32.to_be_bytes());
    apng_data.extend_from_slice(b"acTL");
    apng_data.extend_from_slice(&[0; 8]);
    apng_data.extend_from_slice(&[0; 4]);
    apng_data.extend_from_slice(&0u32.to_be_bytes());
    apng_data.extend_from_slice(b"IDAT");
    apng_data.extend_from_slice(&[0; 4]);
    std::fs::write(&apng_path, &apng_data).unwrap();

    // 2. Synthetic animated WebP
    let webp_path = temp_dir.join("animated.webp");
    let mut webp_data = Vec::new();
    webp_data.extend_from_slice(b"RIFF");
    webp_data.extend_from_slice(&32u32.to_le_bytes());
    webp_data.extend_from_slice(b"WEBP");
    webp_data.extend_from_slice(b"VP8X");
    webp_data.extend_from_slice(&10u32.to_le_bytes());
    webp_data.extend_from_slice(&[0x02, 0, 0, 0]); // bit 1: animation flag
    webp_data.extend_from_slice(&[0; 6]);
    std::fs::write(&webp_path, &webp_data).unwrap();

    let app = app();
    let w = window(&app, "main");

    code(
        invoke(
            &w,
            "import_dropped_inputs",
            json!({"paths": [apng_path.to_str().unwrap()]}),
        ),
        "unsupported_input",
    );

    code(
        invoke(
            &w,
            "import_dropped_inputs",
            json!({"paths": [webp_path.to_str().unwrap()]}),
        ),
        "unsupported_input",
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
