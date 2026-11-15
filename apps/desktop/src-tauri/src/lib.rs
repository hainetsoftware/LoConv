//! Native IPC v1 host. No generic filesystem, shell or network access in JS.
mod dialogs;
mod session;
use loconv_contracts::*;
use loconv_engine::{
    errors::{self, EngineResult},
    paths,
};
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{
    ipc::{InvokeBody, Request},
    Emitter, Manager, State,
};

struct NativeState {
    session: Arc<Mutex<session::Session>>,
    dialogs: Arc<dyn dialogs::Dialogs>,
    active_tokens: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    is_processing: Arc<AtomicBool>,
}

impl Default for NativeState {
    fn default() -> Self {
        Self {
            session: Arc::new(Mutex::default()),
            dialogs: Arc::new(dialogs::NativeDialogs),
            active_tokens: Arc::new(Mutex::default()),
            is_processing: Arc::new(AtomicBool::new(false)),
        }
    }
}

// Parse the entire invoke payload so unknown/missing fields and malformed DTOs
// reject with fixed AppError rather than Tauri's raw deserialization text.
fn args<T: DeserializeOwned>(request: &Request<'_>) -> EngineResult<T> {
    match request.body() {
        InvokeBody::Json(value) => {
            serde_json::from_value(value.clone()).map_err(|_| errors::invalid_request())
        }
        _ => Err(errors::invalid_request()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dropped {
    paths: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enqueue {
    request: ConversionRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cancel {
    job_id: String,
}

#[tauri::command]
fn get_api_version(request: Request<'_>) -> EngineResult<u32> {
    args::<Empty>(&request)?;
    Ok(loconv_engine::api_version())
}

#[tauri::command]
fn list_formats(
    request: Request<'_>,
    state: State<'_, NativeState>,
) -> EngineResult<Vec<FormatDescriptor>> {
    args::<Empty>(&request)?;
    let session = state.session.lock().map_err(|_| errors::internal())?;
    Ok(loconv_engine::formats::descriptors_with_audio(
        session.encoder.as_ref().filter(|_| session.probe.is_some()),
    ))
}

#[tauri::command]
fn list_jobs(
    request: Request<'_>,
    state: State<'_, NativeState>,
) -> EngineResult<Vec<JobSnapshot>> {
    args::<Empty>(&request)?;
    Ok(state
        .session
        .lock()
        .map_err(|_| errors::internal())?
        .jobs
        .list())
}

fn trigger_queue_worker<R: tauri::Runtime>(state: &NativeState, app: tauri::AppHandle<R>) {
    if state
        .is_processing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let session_arc = state.session.clone();
    let is_processing = state.is_processing.clone();
    let active_tokens = state.active_tokens.clone();

    tauri::async_runtime::spawn(async move {
        loop {
            // Find next queued job in session
            let cancel_token = Arc::new(AtomicBool::new(false));
            let next_work = {
                let mut session = match session_arc.lock() {
                    Ok(s) => s,
                    Err(_) => break,
                };

                let queued = session
                    .jobs
                    .list()
                    .into_iter()
                    .find(|j| j.state == JobState::Queued);

                if let Some(mut job) = queued {
                    job.state = JobState::Running;
                    job.revision = match job.revision.checked_add(1) {
                        Some(r) => r,
                        None => break,
                    };
                    job.progress = None;
                    session.jobs.update(job.clone());

                    if let Ok(mut tokens) = active_tokens.lock() {
                        tokens.insert(job.job_id.clone(), cancel_token.clone());
                    }

                    let input = session.inputs.get(&job.media_id).cloned();
                    let dest = session.job_destinations.get(&job.job_id).cloned();

                    (
                        Some(job),
                        input,
                        dest,
                        session.encoder.clone(),
                        session.probe.clone(),
                    )
                } else {
                    // Release the worker gate while holding the same lock used by enqueue.
                    // A subsequent enqueue must either be seen here or start a new worker.
                    is_processing.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let (Some(job), Some(input), Some(dest), encoder, probe) = next_work else {
                break;
            };

            // Emit Running state event
            let _ = app.emit_to(
                "main",
                "loconv://job-changed",
                JobEvent {
                    api_version: API_VERSION,
                    job: job.clone(),
                },
            );

            // Spawn blocking task for conversion
            let input_path = input.path.clone();
            let final_target_path = dest.join(&job.output_name);
            let temp_target_path = dest.join(format!(".loconv-tmp-{}.tmp", uuid::Uuid::new_v4()));
            let cancel_flag = cancel_token.clone();
            let format_id = job.format_id.clone();
            let temp_path_for_worker = temp_target_path.clone();

            let temp_owned = Arc::new(AtomicBool::new(false));
            let worker_owned = temp_owned.clone();
            let progress_session = session_arc.clone();
            let progress_app = app.clone();
            let progress_id = job.job_id.clone();
            let run_res = tauri::async_runtime::spawn_blocking(move || {
                let source = input.identity.open(&input_path)?;
                let mut options = std::fs::OpenOptions::new();
                options.read(true).write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let output = options
                    .open(&temp_path_for_worker)
                    .map_err(|_| errors::conversion_failed())?;
                worker_owned.store(true, Ordering::SeqCst);
                let verification = output.try_clone().map_err(|_| errors::internal())?;
                if input.inspection.kind == MediaKind::Audio {
                    let encoder = encoder.ok_or_else(errors::tool_unavailable)?;
                    let probe = probe.ok_or_else(errors::tool_unavailable)?;
                    encoder.convert_to_file(
                        loconv_engine::backends::ffmpeg::AudioRequest {
                            input: source,
                            output: &temp_path_for_worker,
                            inspection: &input.inspection,
                            format: &format_id,
                            probe: &probe,
                            cancel: &cancel_flag,
                        },
                        output,
                        move |value| {
                            if let Ok(mut session) = progress_session.lock() {
                                if let Some(mut current) = session.jobs.get(&progress_id).cloned() {
                                    if current.state == JobState::Running
                                        && current.progress.is_none_or(|v| value > v)
                                    {
                                        current.progress = Some(value);
                                        current.revision += 1;
                                        session.jobs.update(current.clone());
                                        let _ = progress_app.emit_to(
                                            "main",
                                            "loconv://job-changed",
                                            JobEvent {
                                                api_version: API_VERSION,
                                                job: current,
                                            },
                                        );
                                    }
                                }
                            }
                        },
                    )
                } else {
                    loconv_engine::backends::image::convert_image_to_file(
                        &input_path,
                        &temp_path_for_worker,
                        &format_id,
                        &cancel_flag,
                        output,
                    )
                }?;
                paths::SourceIdentity::capture(
                    &verification
                        .metadata()
                        .map_err(|_| errors::conversion_failed())?,
                )
            })
            .await;

            // Clean up token
            {
                if let Ok(mut tokens) = active_tokens.lock() {
                    tokens.remove(&job.job_id);
                }
            }

            // Update final state in session and emit.
            // Cancellation and publication are serialized under the session lock:
            // if cancellation is accepted first, no final output is published.
            let final_snapshot = {
                let mut session = match session_arc.lock() {
                    Ok(s) => s,
                    Err(_) => {
                        let _ = std::fs::remove_file(&temp_target_path);
                        break;
                    }
                };

                // Free reservation
                session
                    .reserved_targets
                    .remove(&dest.join(&job.output_name));
                session.job_destinations.remove(&job.job_id);

                let mut current = session.jobs.get(&job.job_id).cloned().unwrap_or(job);

                if !loconv_engine::jobs::is_terminal(&current.state) {
                    let cancelled = current.state == JobState::Cancelling
                        || cancel_token.load(Ordering::SeqCst);
                    let result = match run_res {
                        Ok(result) => result,
                        Err(_) => Err(errors::internal()),
                    };
                    let cleanup = || {
                        if temp_owned.load(Ordering::SeqCst) {
                            paths::cleanup_temp(&temp_target_path)
                        } else {
                            Ok(())
                        }
                    };
                    let result = if cancelled {
                        cleanup()
                    } else {
                        match result {
                            Ok(identity) => match identity.open(&temp_target_path) {
                                Ok(_) => paths::atomic_publish_no_overwrite(
                                    &temp_target_path,
                                    &final_target_path,
                                ),
                                Err(err) => cleanup().and(Err(err)),
                            },
                            Err(err) => cleanup().and(Err(err)),
                        }
                    };
                    current.revision += 1;
                    current.progress = None;
                    match result {
                        Ok(()) if cancelled => {
                            current.state = JobState::Cancelled;
                            current.error = None;
                        }
                        Ok(()) => {
                            current.state = JobState::Succeeded;
                            current.progress = Some(1.0);
                            current.error = None;
                        }
                        Err(err) => {
                            current.state = JobState::Failed;
                            current.error = Some(err);
                        }
                    }
                }

                session.jobs.update(current.clone());
                current
            };

            let _ = app.emit_to(
                "main",
                "loconv://job-changed",
                JobEvent {
                    api_version: API_VERSION,
                    job: final_snapshot,
                },
            );
        }

        is_processing.store(false, Ordering::SeqCst);
    });
}

#[tauri::command]
fn enqueue_job<R: tauri::Runtime>(
    request: Request<'_>,
    state: State<'_, NativeState>,
    app: tauri::AppHandle<R>,
) -> EngineResult<JobSnapshot> {
    let payload: Enqueue = args(&request)?;
    let snapshot = state
        .session
        .lock()
        .map_err(|_| errors::internal())?
        .enqueue(payload.request)?;

    trigger_queue_worker(&state, app);

    Ok(snapshot)
}

#[tauri::command]
fn cancel_job<R: tauri::Runtime>(
    request: Request<'_>,
    state: State<'_, NativeState>,
    app: tauri::AppHandle<R>,
) -> EngineResult<JobSnapshot> {
    let payload: Cancel = args(&request)?;
    if payload.job_id.is_empty() {
        return Err(errors::invalid_request());
    }

    let mut session = state.session.lock().map_err(|_| errors::internal())?;
    let job = session
        .jobs
        .list()
        .into_iter()
        .find(|j| j.job_id == payload.job_id)
        .ok_or_else(|| AppError {
            code: ErrorCode::JobNotFound,
            message: "The job was not found in this session.".into(),
            retryable: false,
        })?;

    if loconv_engine::jobs::is_terminal(&job.state) {
        return Ok(job);
    }

    if job.state == JobState::Queued {
        let updated = session.jobs.cancel(&payload.job_id)?;
        if let Some(dest) = session.job_destinations.remove(&payload.job_id) {
            session
                .reserved_targets
                .remove(&dest.join(&updated.output_name));
        }
        app.emit_to(
            "main",
            "loconv://job-changed",
            JobEvent {
                api_version: API_VERSION,
                job: updated.clone(),
            },
        )
        .map_err(|_| errors::internal())?;
        return Ok(updated);
    }

    if job.state == JobState::Running {
        if let Some(token) = state
            .active_tokens
            .lock()
            .map_err(|_| errors::internal())?
            .get(&payload.job_id)
        {
            token.store(true, Ordering::SeqCst);
        }
        let updated = session.jobs.cancel(&payload.job_id)?;
        app.emit_to(
            "main",
            "loconv://job-changed",
            JobEvent {
                api_version: API_VERSION,
                job: updated.clone(),
            },
        )
        .map_err(|_| errors::internal())?;
        return Ok(updated);
    }

    if job.state == JobState::Cancelling {
        return Ok(job);
    }

    Ok(job)
}

#[tauri::command]
async fn import_dropped_inputs(
    request: Request<'_>,
    state: State<'_, NativeState>,
) -> EngineResult<Vec<InputInspection>> {
    let payload: Dropped = args(&request)?;
    import_paths(state.session.clone(), payload.paths).await
}

async fn import_paths(
    session: Arc<Mutex<session::Session>>,
    paths: Vec<String>,
) -> EngineResult<Vec<InputInspection>> {
    tauri::async_runtime::spawn_blocking(move || {
        session
            .lock()
            .map_err(|_| errors::internal())?
            .import(paths)
    })
    .await
    .map_err(|_| errors::internal())?
}

async fn choose<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    dialogs: Arc<dyn dialogs::Dialogs>,
    directory: bool,
) -> EngineResult<Vec<std::path::PathBuf>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(dialogs.choose(directory));
    })
    .map_err(|_| errors::internal())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|_| errors::internal()))
        .await
        .map_err(|_| errors::internal())??
}

#[tauri::command]
async fn select_inputs<R: tauri::Runtime>(
    request: Request<'_>,
    state: State<'_, NativeState>,
    app: tauri::AppHandle<R>,
) -> EngineResult<Vec<InputInspection>> {
    args::<Empty>(&request)?;
    let paths = choose(app, state.dialogs.clone(), false).await?;
    let paths = paths
        .into_iter()
        .map(|p| {
            p.into_os_string()
                .into_string()
                .map_err(|_| errors::invalid_request())
        })
        .collect::<EngineResult<Vec<_>>>()?;
    import_paths(state.session.clone(), paths).await
}

#[tauri::command]
async fn select_destination<R: tauri::Runtime>(
    request: Request<'_>,
    state: State<'_, NativeState>,
    app: tauri::AppHandle<R>,
) -> EngineResult<Option<DestinationSelection>> {
    args::<Empty>(&request)?;
    let paths = choose(app, state.dialogs.clone(), true).await?;
    if paths.len() > 1 {
        return Err(errors::invalid_request());
    }
    let selected = paths.into_iter().next();
    let selected = tauri::async_runtime::spawn_blocking(move || {
        selected.map(session::prepare_destination).transpose()
    })
    .await
    .map_err(|_| errors::internal())??;
    state
        .session
        .lock()
        .map_err(|_| errors::internal())?
        .destination(selected)
}

fn configure<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    state: NativeState,
) -> tauri::Builder<R> {
    builder
        .manage(state)
        .setup(|app| {
            if let Ok(resources) = app.path().resource_dir() {
                let probe = loconv_engine::backends::ffmpeg::TrustedProbe::resolve(&resources).ok();
                let encoder =
                    loconv_engine::backends::ffmpeg::TrustedFfmpeg::resolve(&resources).ok();
                if let Ok(mut session) = app.state::<NativeState>().session.lock() {
                    session.probe = probe;
                    session.encoder = encoder;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_api_version,
            select_inputs,
            import_dropped_inputs,
            select_destination,
            list_formats,
            enqueue_job,
            list_jobs,
            cancel_job
        ])
}

fn context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure(tauri::Builder::default(), NativeState::default())
        .run(context())
        .expect("failed to run LoConv");
}

#[cfg(test)]
mod tests;
