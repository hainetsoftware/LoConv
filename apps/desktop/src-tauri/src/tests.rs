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
    let app = configure(
        mock_builder(),
        NativeState {
            dialogs: Arc::new(CancelledDialogs),
            ..Default::default()
        },
    )
    .build(context())
    .unwrap();
    // General IPC tests have deterministic unavailable tools even in opt-in runs.
    {
        let state = app.state::<NativeState>();
        let mut session = state.session.lock().unwrap();
        session.encoder = None;
        session.probe = None;
    }
    app
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

    // 3. Synthetic oversized PNG (> 100 megapixels)
    let oversized_path = temp_dir.join("oversized.png");
    let mut oversized_data = Vec::new();
    oversized_data.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // signature
    oversized_data.extend_from_slice(&13u32.to_be_bytes()); // IHDR length = 13
    let mut ihdr_data = Vec::new();
    ihdr_data.extend_from_slice(b"IHDR");
    ihdr_data.extend_from_slice(&15000u32.to_be_bytes()); // width 15000
    ihdr_data.extend_from_slice(&10000u32.to_be_bytes()); // height 10000 (total = 150 MP > 100 MP limit)
    ihdr_data.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB, deflate, no filter, no interlace
    let mut crc = 0xFFFFFFFFu32;
    for &b in &ihdr_data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    oversized_data.extend_from_slice(&ihdr_data); // Chunk type + IHDR payload
    oversized_data.extend_from_slice(&(!crc).to_be_bytes()); // CRC
                                                             // Minimal empty IDAT chunk
    let mut idat_crc = 0xFFFFFFFFu32;
    for &b in b"IDAT" {
        idat_crc ^= b as u32;
        for _ in 0..8 {
            if idat_crc & 1 != 0 {
                idat_crc = (idat_crc >> 1) ^ 0xEDB88320;
            } else {
                idat_crc >>= 1;
            }
        }
    }
    oversized_data.extend_from_slice(&0u32.to_be_bytes());
    oversized_data.extend_from_slice(b"IDAT");
    oversized_data.extend_from_slice(&(!idat_crc).to_be_bytes());
    // Minimal empty IEND chunk
    let mut iend_crc = 0xFFFFFFFFu32;
    for &b in b"IEND" {
        iend_crc ^= b as u32;
        for _ in 0..8 {
            if iend_crc & 1 != 0 {
                iend_crc = (iend_crc >> 1) ^ 0xEDB88320;
            } else {
                iend_crc >>= 1;
            }
        }
    }
    oversized_data.extend_from_slice(&0u32.to_be_bytes());
    oversized_data.extend_from_slice(b"IEND");
    oversized_data.extend_from_slice(&(!iend_crc).to_be_bytes());
    std::fs::write(&oversized_path, &oversized_data).unwrap();

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

    code(
        invoke(
            &w,
            "import_dropped_inputs",
            json!({"paths": [oversized_path.to_str().unwrap()]}),
        ),
        "resource_limit",
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn real_synthetic_image_conversion_all_combinations_e2e() {
    let temp_dir = std::env::temp_dir().join(format!("loconv-allcomb-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let dest_dir = temp_dir.join("dest");
    std::fs::create_dir_all(&dest_dir).unwrap();

    // Create synthetic input images: PNG, JPEG, WebP
    let png_path = temp_dir.join("test.png");
    let jpeg_path = temp_dir.join("test.jpg");
    let webp_path = temp_dir.join("test.webp");

    let img = image::RgbImage::from_fn(20, 15, |x, y| {
        image::Rgb([(x * 10) as u8, (y * 12) as u8, 180])
    });
    img.save_with_format(&png_path, image::ImageFormat::Png)
        .unwrap();
    img.save_with_format(&jpeg_path, image::ImageFormat::Jpeg)
        .unwrap();
    img.save_with_format(&webp_path, image::ImageFormat::WebP)
        .unwrap();

    let png_bytes = std::fs::read(&png_path).unwrap();
    let jpeg_bytes = std::fs::read(&jpeg_path).unwrap();
    let webp_bytes = std::fs::read(&webp_path).unwrap();

    struct AllCombDialogs {
        inputs: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
    }
    impl dialogs::Dialogs for AllCombDialogs {
        fn choose(&self, directory: bool) -> EngineResult<Vec<std::path::PathBuf>> {
            if directory {
                Ok(vec![self.dest.clone()])
            } else {
                Ok(self.inputs.clone())
            }
        }
    }

    let app = configure(
        mock_builder(),
        NativeState {
            dialogs: Arc::new(AllCombDialogs {
                inputs: vec![png_path.clone(), jpeg_path.clone(), webp_path.clone()],
                dest: dest_dir.clone(),
            }),
            ..Default::default()
        },
    )
    .build(context())
    .unwrap();
    let w = window(&app, "main");

    let imported = invoke(&w, "select_inputs", json!({})).unwrap();
    let imported_arr = imported.as_array().unwrap();
    assert_eq!(imported_arr.len(), 3);

    let png_media_id = imported_arr
        .iter()
        .find(|m| m["container"] == "png")
        .unwrap()["mediaId"]
        .as_str()
        .unwrap();
    let jpeg_media_id = imported_arr
        .iter()
        .find(|m| m["container"] == "jpeg")
        .unwrap()["mediaId"]
        .as_str()
        .unwrap();
    let webp_media_id = imported_arr
        .iter()
        .find(|m| m["container"] == "webp")
        .unwrap()["mediaId"]
        .as_str()
        .unwrap();

    let dest_sel = invoke(&w, "select_destination", json!({})).unwrap();
    let dest_id = dest_sel["destinationId"].as_str().unwrap();

    // Test combinations:
    // 1. PNG -> WebP
    // 2. JPEG -> PNG
    // 3. WebP -> JPEG
    let test_cases = [
        (png_media_id, "webp", "out_from_png.webp"),
        (jpeg_media_id, "png", "out_from_jpeg.png"),
        (webp_media_id, "jpeg", "out_from_webp.jpg"),
    ];

    for (media_id, format_id, out_name) in test_cases {
        let enqueue_res = invoke(
            &w,
            "enqueue_job",
            json!({
                "request": {
                    "mediaId": media_id,
                    "destinationId": dest_id,
                    "formatId": format_id,
                    "outputName": out_name
                }
            }),
        )
        .unwrap();

        assert_eq!(enqueue_res["state"], "queued");

        let out_file = dest_dir.join(out_name);
        let start = std::time::Instant::now();
        while !out_file.exists() && start.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(out_file.exists(), "Output {out_name} must be published");

        // Verify readable with correct dimensions
        let decoded = image::open(&out_file).expect("Output must be readable image");
        assert_eq!(decoded.width(), 20);
        assert_eq!(decoded.height(), 15);
    }

    // Verify source files are untouched
    assert_eq!(std::fs::read(&png_path).unwrap(), png_bytes);
    assert_eq!(std::fs::read(&jpeg_path).unwrap(), jpeg_bytes);
    assert_eq!(std::fs::read(&webp_path).unwrap(), webp_bytes);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[cfg(all(debug_assertions, feature = "dev-tools", target_os = "macos"))]
#[test]
#[ignore = "real pinned FFprobe integration; native handlers with synthetic picker"]
fn real_audio_picker_and_drop_inspection() {
    let probe = loconv_engine::backends::ffmpeg::TrustedProbe::resolve(std::path::Path::new(
        "/unused-resource-root",
    ))
    .unwrap();
    let dir = std::env::temp_dir().join(format!("loconv-native-audio-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("generated.bin");
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&1636u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&1600u32.to_le_bytes());
    wav.resize(1644, 0);
    std::fs::write(&path, &wav).unwrap();
    struct SyntheticPicker(std::path::PathBuf);
    impl dialogs::Dialogs for SyntheticPicker {
        fn choose(&self, directory: bool) -> EngineResult<Vec<std::path::PathBuf>> {
            assert!(!directory);
            Ok(vec![self.0.clone()])
        }
    }
    let app = configure(
        mock_builder(),
        NativeState {
            dialogs: Arc::new(SyntheticPicker(path.clone())),
            ..Default::default()
        },
    )
    .build(context())
    .unwrap();
    app.state::<NativeState>().session.lock().unwrap().probe = Some(probe);
    app.state::<NativeState>().session.lock().unwrap().encoder = None;
    let w = window(&app, "main");
    for (command, body) in [
        ("select_inputs", json!({})),
        ("import_dropped_inputs", json!({"paths":[path]})),
    ] {
        let result = invoke(&w, command, body).unwrap();
        assert_eq!(result[0]["kind"], "audio");
        assert_eq!(result[0]["container"], "wav");
        assert_eq!(result[0]["streams"][0]["codec"], "pcm_s16le");
        assert_eq!(result[0]["sizeBytes"], "1644");
        assert!(uuid::Uuid::parse_str(result[0]["mediaId"].as_str().unwrap()).is_ok());
        code(
            invoke(
                &w,
                "enqueue_job",
                json!({"request":{"mediaId":result[0]["mediaId"], "destinationId":"unused", "formatId":"mp3", "outputName":"generated.mp3"}}),
            ),
            "tool_unavailable",
        );
    }
    // A batch containing a playlist must register none of its staged inputs.
    let playlist = dir.join("invented.wav");
    std::fs::write(&playlist, b"#EXTM3U\nhttps://example.invalid/a").unwrap();
    code(
        invoke(
            &w,
            "import_dropped_inputs",
            json!({"paths":[path, playlist]}),
        ),
        "unsupported_input",
    );
    assert_eq!(
        app.state::<NativeState>()
            .session
            .lock()
            .unwrap()
            .inputs
            .len(),
        2
    );
    assert_eq!(std::fs::read(&path).unwrap(), wav);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(all(debug_assertions, feature = "dev-tools", target_os = "macos"))]
#[test]
#[ignore = "requires explicitly pinned development tools; synthetic native audio queue"]
fn real_audio_native_queue() {
    use std::time::{Duration, Instant};
    let root = std::path::Path::new("/unused-resource-root");
    let probe = loconv_engine::backends::ffmpeg::TrustedProbe::resolve(root).unwrap();
    let encoder = loconv_engine::backends::ffmpeg::TrustedFfmpeg::resolve(root).unwrap();
    let dir = std::env::temp_dir().join(format!("loconv-native-encode-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let input = dir.join("generated.wav");
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&96036u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48000u32.to_le_bytes());
    wav.extend_from_slice(&96000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&96000u32.to_le_bytes());
    wav.resize(96044, 0);
    std::fs::write(&input, &wav).unwrap();
    let image_input = dir.join("generated.png");
    image::RgbImage::new(8, 8).save(&image_input).unwrap();
    let app = app();
    {
        let state = app.state::<NativeState>();
        let mut session = state.session.lock().unwrap();
        session.probe = Some(probe);
        session.encoder = Some(encoder);
    }
    let w = window(&app, "main");
    let selection = session::prepare_destination(dir.clone()).unwrap();
    let destination = app
        .state::<NativeState>()
        .session
        .lock()
        .unwrap()
        .destination(Some(selection))
        .unwrap()
        .unwrap();
    let formats = invoke(&w, "list_formats", json!({})).unwrap();
    for format in ["mp3", "wav_pcm16"] {
        assert_eq!(
            formats
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["id"] == format)
                .unwrap()["available"],
            true
        );
    }
    let mut jobs = Vec::new();
    for (path, format, name) in [
        (&input, "mp3", "converted.mp3"),
        (&image_input, "webp", "converted.webp"),
        (&input, "wav_pcm16", "converted.wav"),
    ] {
        let imported = invoke(&w, "import_dropped_inputs", json!({"paths":[path]})).unwrap();
        let queued = invoke(&w, "enqueue_job", json!({"request":{"mediaId":imported[0]["mediaId"],"destinationId":destination.destination_id,"formatId":format,"outputName":name}})).unwrap();
        jobs.push(queued["jobId"].as_str().unwrap().to_owned());
    }
    let start = Instant::now();
    loop {
        let list = invoke(&w, "list_jobs", json!({})).unwrap();
        let states = list.as_array().unwrap();
        assert!(states.iter().filter(|j| j["state"] == "running").count() <= 1);
        if states.iter().all(|j| j["state"] == "succeeded") {
            break;
        }
        assert!(!states.iter().any(|j| j["state"] == "failed"));
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(5));
    }
    for id in jobs {
        let completed = invoke(&w, "cancel_job", json!({"jobId":id})).unwrap();
        assert_eq!(completed["state"], "succeeded");
        assert_eq!(completed["progress"], 1.0);
    }
    assert_eq!(std::fs::read(&input).unwrap(), wav);
    assert!(app
        .state::<NativeState>()
        .session
        .lock()
        .unwrap()
        .reserved_targets
        .is_empty());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 5);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn concurrent_enqueue_cancel_and_worker_restart() {
    use std::time::{Duration, Instant};
    let dir = std::env::temp_dir().join(format!("loconv-queue-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let input = dir.join("synthetic.png");
    image::RgbImage::new(32, 32).save(&input).unwrap();
    let app = app();
    let w = window(&app, "main");
    let imported = invoke(&w, "import_dropped_inputs", json!({"paths":[input]})).unwrap();
    let media = imported[0]["mediaId"].as_str().unwrap().to_owned();
    let destination = app
        .state::<NativeState>()
        .session
        .lock()
        .unwrap()
        .destination(Some(session::prepare_destination(dir.clone()).unwrap()))
        .unwrap()
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8).map(|index| {
        let w = w.clone(); let media = media.clone(); let destination = destination.destination_id.clone(); let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let job = invoke(&w, "enqueue_job", json!({"request":{"mediaId":media,"destinationId":destination,"formatId":"webp","outputName":format!("synthetic-{index}.webp")}})).unwrap();
            let cancelled = invoke(&w, "cancel_job", json!({"jobId":job["jobId"]})).unwrap();
            let again = invoke(&w, "cancel_job", json!({"jobId":job["jobId"]})).unwrap();
            assert!(again["revision"].as_u64().unwrap() >= cancelled["revision"].as_u64().unwrap());
            job["jobId"].as_str().unwrap().to_owned()
        })
    }).collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let start = Instant::now();
    loop {
        let list = invoke(&w, "list_jobs", json!({})).unwrap();
        let jobs = list.as_array().unwrap();
        assert!(jobs.iter().filter(|j| j["state"] == "running").count() <= 1);
        if jobs
            .iter()
            .all(|j| j["state"] == "succeeded" || j["state"] == "cancelled")
        {
            for job in jobs {
                assert_eq!(
                    dir.join(job["outputName"].as_str().unwrap()).exists(),
                    job["state"] == "succeeded"
                );
            }
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    // Repeatedly enqueue after an idle worker exits, exercising the worker gate.
    for index in 0..8 {
        let job = invoke(&w, "enqueue_job", json!({"request":{"mediaId":media,"destinationId":destination.destination_id,"formatId":"png","outputName":format!("restart-{index}.png")}})).unwrap();
        let start = Instant::now();
        while app
            .state::<NativeState>()
            .session
            .lock()
            .unwrap()
            .jobs
            .get(job["jobId"].as_str().unwrap())
            .unwrap()
            .state
            != JobState::Succeeded
        {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // Hold only the worker gate to stage deterministic failed/queued-cancelled work.
    let start = Instant::now();
    while app
        .state::<NativeState>()
        .is_processing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    let request = |name: &str, media: &str| json!({"request":{"mediaId":media,"destinationId":destination.destination_id,"formatId":"png","outputName":name}});
    let failed = invoke(&w, "enqueue_job", request("failed.png", &media)).unwrap();
    let cancelled_request = request("cancelled.png", &media);
    let cancelled = invoke(&w, "enqueue_job", cancelled_request.clone()).unwrap();
    code(
        invoke(&w, "enqueue_job", cancelled_request),
        "output_conflict",
    );
    let once = invoke(&w, "cancel_job", json!({"jobId":cancelled["jobId"]})).unwrap();
    assert_eq!(once["state"], "cancelled");
    assert_eq!(
        invoke(&w, "cancel_job", json!({"jobId":cancelled["jobId"]})).unwrap(),
        once
    );
    let healthy = dir.join("healthy.png");
    image::RgbImage::new(8, 8).save(&healthy).unwrap();
    let imported = invoke(&w, "import_dropped_inputs", json!({"paths":[healthy]})).unwrap();
    let continuation = invoke(
        &w,
        "enqueue_job",
        request("continued.png", imported[0]["mediaId"].as_str().unwrap()),
    )
    .unwrap();
    std::fs::write(&input, b"changed synthetic source").unwrap();
    let state = app.state::<NativeState>();
    state.is_processing.store(false, Ordering::SeqCst);
    trigger_queue_worker(&state, app.handle().clone());
    let start = Instant::now();
    while state
        .session
        .lock()
        .unwrap()
        .jobs
        .get(continuation["jobId"].as_str().unwrap())
        .unwrap()
        .state
        != JobState::Succeeded
    {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    }
    let failed = state
        .session
        .lock()
        .unwrap()
        .jobs
        .get(failed["jobId"].as_str().unwrap())
        .unwrap()
        .clone();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.error.unwrap().code, ErrorCode::InputUnavailable);
    assert!(!dir.join("failed.png").exists());
    assert!(!dir.join("cancelled.png").exists());
    assert!(app
        .state::<NativeState>()
        .session
        .lock()
        .unwrap()
        .reserved_targets
        .is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}
