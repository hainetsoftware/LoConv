use loconv_contracts::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

fn round_trip<T: DeserializeOwned + Serialize>(source: &str) -> T {
    let expected: Value = serde_json::from_str(source).unwrap();
    let model: T = serde_json::from_str(source).unwrap();
    assert_eq!(serde_json::to_value(&model).unwrap(), expected);
    model
}

#[test]
fn synthetic_shared_fixtures_preserve_wire_shape() {
    let inspection =
        round_trip::<InputInspection>(include_str!("../../../contracts/fixtures/inspection.json"));
    assert_eq!(inspection.size_bytes, "9007199254740993");
    assert_eq!(inspection.streams[0].width, None);
    round_trip::<ConversionRequest>(include_str!("../../../contracts/fixtures/request.json"));
    let event = round_trip::<JobEvent>(include_str!("../../../contracts/fixtures/job-event.json"));
    assert_eq!(event.api_version, API_VERSION);
    assert_eq!(event.job.state, JobState::Failed);
    assert_eq!(event.job.error.unwrap().code, ErrorCode::ToolUnavailable);
    let formats = round_trip::<Vec<FormatDescriptor>>(include_str!(
        "../../../contracts/fixtures/formats.json"
    ));
    assert_eq!(formats.len(), 7);
}

#[test]
fn requests_reject_arbitrary_flags_and_unknown_formats() {
    let mut request: Value =
        serde_json::from_str(include_str!("../../../contracts/fixtures/request.json")).unwrap();
    request["ffmpegArgs"] = json!(["-i", "https://invalid.example"]);
    assert!(serde_json::from_value::<ConversionRequest>(request.clone()).is_err());
    request.as_object_mut().unwrap().remove("ffmpegArgs");
    request["formatId"] = json!("custom");
    assert!(serde_json::from_value::<ConversionRequest>(request).is_err());
}

#[test]
fn all_job_states_have_stable_spelling() {
    for (state, wire) in [
        (JobState::Queued, "queued"),
        (JobState::Running, "running"),
        (JobState::Cancelling, "cancelling"),
        (JobState::Succeeded, "succeeded"),
        (JobState::Failed, "failed"),
        (JobState::Cancelled, "cancelled"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(wire));
    }
}
