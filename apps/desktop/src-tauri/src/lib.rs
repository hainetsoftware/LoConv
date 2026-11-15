//! Native IPC v1 host. No generic filesystem, shell or network access in JS.
mod dialogs;
mod session;
use loconv_contracts::*;
use loconv_engine::errors::{self, EngineResult};
use serde::{de::DeserializeOwned, Deserialize};
use std::sync::{Arc, Mutex};
use tauri::{
    ipc::{InvokeBody, Request},
    Emitter, State,
};

struct NativeState {
    session: Mutex<session::Session>,
    dialogs: Arc<dyn dialogs::Dialogs>,
}
impl Default for NativeState {
    fn default() -> Self {
        Self {
            session: Mutex::default(),
            dialogs: Arc::new(dialogs::NativeDialogs),
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
fn list_formats(request: Request<'_>) -> EngineResult<Vec<FormatDescriptor>> {
    args::<Empty>(&request)?;
    Ok(loconv_engine::formats::descriptors())
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
#[tauri::command]
fn enqueue_job(request: Request<'_>, state: State<'_, NativeState>) -> EngineResult<JobSnapshot> {
    let payload: Enqueue = args(&request)?;
    state
        .session
        .lock()
        .map_err(|_| errors::internal())?
        .enqueue(payload.request)
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
    // Serialize mutation and publication together; do not hold a lock across I/O.
    let mut session = state.session.lock().map_err(|_| errors::internal())?;
    let before = session
        .jobs
        .list()
        .into_iter()
        .find(|j| j.job_id == payload.job_id);
    let job = session.jobs.cancel(&payload.job_id)?;
    if before
        .as_ref()
        .is_some_and(|old| old.revision < job.revision)
    {
        app.emit_to(
            "main",
            "loconv://job-changed",
            JobEvent {
                api_version: API_VERSION,
                job: job.clone(),
            },
        )
        .map_err(|_| errors::internal())?;
    }
    Ok(job)
}
#[tauri::command]
fn import_dropped_inputs(request: Request<'_>) -> EngineResult<Vec<InputInspection>> {
    let payload: Dropped = args(&request)?;
    session::import(payload.paths)
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
    session::import(paths)
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
