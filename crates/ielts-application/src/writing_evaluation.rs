use ielts_db::{
    DeterministicProvider, EvaluationHandle, EvaluationRunResult, PreparedEvaluation,
    ProviderError, StartEvaluationCommand, WritingProvider,
};
use ielts_domain::dto::{WritingFeedbackV4, WritingScoreV4};

use crate::{
    ApplicationError, ChatMessage, CompletionRequest, EventSink, LanguageModel, ModelError,
    WritingEvaluationStore,
};

#[derive(Debug)]
pub struct StartEvaluationOutcome {
    pub handle: EvaluationHandle,
    pub pending: Option<PreparedEvaluation>,
}

pub enum EvaluationBackend<M> {
    Deterministic,
    Language(M),
    Unavailable(ModelError),
}

#[derive(Debug)]
struct ProviderOutput {
    score: WritingScoreV4,
    feedback: Result<WritingFeedbackV4, ProviderError>,
}

pub struct WritingEvaluationService;

impl WritingEvaluationService {
    pub fn start<S: WritingEvaluationStore, E: EventSink + ?Sized>(
        store: &S,
        command: &StartEvaluationCommand,
        provider_id: &str,
        model: &str,
        events: &E,
    ) -> Result<StartEvaluationOutcome, ApplicationError> {
        let prepared = store.prepare(command, provider_id, model)?;
        let handle = prepared.handle.clone();

        if let Some(existing) = prepared.existing.as_ref() {
            emit_all(events, existing.events.iter().cloned());
            return Ok(StartEvaluationOutcome {
                handle,
                pending: None,
            });
        }

        if let Ok(initial_events) = store.list_events(&handle.evaluation_id, 0) {
            emit_all(events, initial_events);
        }

        Ok(StartEvaluationOutcome {
            handle,
            pending: Some(prepared),
        })
    }

    pub async fn execute<S: WritingEvaluationStore, M: LanguageModel, E: EventSink + ?Sized>(
        store: &S,
        prepared: PreparedEvaluation,
        backend: EvaluationBackend<M>,
        events: &E,
    ) -> Result<EvaluationRunResult, ApplicationError> {
        let sequence = prepared.handle.sequence;
        let provider_result = match backend {
            EvaluationBackend::Deterministic => run_deterministic(&prepared),
            EvaluationBackend::Language(model) => evaluate_language_model(&model, &prepared).await,
            EvaluationBackend::Unavailable(error) => Err(error.into()),
        };

        let (score, feedback, review_error) = match provider_result {
            Ok(output) => match output.feedback {
                Ok(feedback) => (Ok(output.score), Some(feedback), None),
                Err(error) => (Ok(output.score), None, Some(error)),
            },
            Err(error) => (Err(error), None, None),
        };

        let result = store.finish(&prepared, score, feedback, review_error)?;
        emit_all(
            events,
            result
                .events
                .iter()
                .filter(|event| event.sequence > sequence)
                .cloned(),
        );
        Ok(result)
    }
}

fn run_deterministic(prepared: &PreparedEvaluation) -> Result<ProviderOutput, ProviderError> {
    let provider = DeterministicProvider;
    let score = provider.score(
        &prepared.essay,
        prepared.prompt.as_deref(),
        prepared.task_type,
    )?;
    let feedback = provider.review(&prepared.essay, &score);
    Ok(ProviderOutput { score, feedback })
}

async fn evaluate_language_model<M: LanguageModel>(
    model: &M,
    prepared: &PreparedEvaluation,
) -> Result<ProviderOutput, ProviderError> {
    let task_type = prepared
        .task_type
        .map(|value| format!("{value:?}"))
        .unwrap_or_default();
    let prompt = format!(
        "Assess this IELTS writing response. Task type: {task_type}. Task prompt: {}\n\nEssay:\n{}",
        prepared.prompt.as_deref().unwrap_or("not provided"),
        prepared.essay
    );
    let response = model
        .complete(CompletionRequest {
            messages: vec![
                ChatMessage::new("system", prepared.system_prompt.clone()),
                ChatMessage::new("user", prompt),
            ],
            temperature: prepared.temperature,
            max_tokens: 12_288,
            thinking: true,
            reasoning_effort: Some("high".into()),
        })
        .await?;
    parse_output(&response.content)
}

fn parse_output(content: &str) -> Result<ProviderOutput, ProviderError> {
    let trimmed = content.trim();
    let json = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .strip_suffix("```")
        .unwrap_or(trimmed)
        .trim();
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| provider_error(format!("AI evaluation JSON invalid: {error}"), false))?;
    let score: WritingScoreV4 = serde_json::from_value(
        value
            .get("score")
            .cloned()
            .ok_or_else(|| provider_error("AI evaluation score is missing", false))?,
    )
    .map_err(|error| provider_error(format!("AI evaluation score invalid: {error}"), false))?;
    validate_score(&score)?;
    let feedback = value
        .get("feedback")
        .cloned()
        .ok_or_else(|| provider_error("AI evaluation feedback is missing", true))
        .and_then(|mut feedback| {
            normalize_feedback_strings(&mut feedback);
            serde_json::from_value(feedback).map_err(|error| {
                provider_error(format!("AI evaluation feedback invalid: {error}"), true)
            })
        });
    Ok(ProviderOutput { score, feedback })
}

/// Providers sometimes preserve the requested feedback content but wrap a
/// nominal string in a small JSON object (for example one key per IELTS
/// criterion). Keep that useful content instead of degrading the entire
/// sentence/paragraph review because of a harmless shape variation.
fn normalize_feedback_strings(feedback: &mut serde_json::Value) {
    let Some(object) = feedback.as_object_mut() else {
        return;
    };

    normalize_string_field(object, "overall");
    normalize_string_array(object.get_mut("plan"));
    normalize_string_array(object.get_mut("rewrites"));

    if let Some(paragraphs) = object
        .get_mut("paragraphs")
        .and_then(|value| value.as_array_mut())
    {
        for paragraph in paragraphs {
            let Some(paragraph) = paragraph.as_object_mut() else {
                continue;
            };
            normalize_string_field(paragraph, "summary");
            normalize_string_array(paragraph.get_mut("issues"));
        }
    }

    if let Some(sentences) = object
        .get_mut("sentences")
        .and_then(|value| value.as_array_mut())
    {
        for sentence in sentences {
            let Some(sentence) = sentence.as_object_mut() else {
                continue;
            };
            for field in ["sentence", "correction", "kind"] {
                normalize_string_field(sentence, field);
            }
        }
    }
}

fn normalize_string_field(object: &mut serde_json::Map<String, serde_json::Value>, field: &str) {
    let Some(value) = object.get_mut(field) else {
        return;
    };
    if !value.is_string() && !value.is_null() {
        *value = serde_json::Value::String(feedback_text(value));
    }
}

fn normalize_string_array(value: Option<&mut serde_json::Value>) {
    let Some(items) = value.and_then(|value| value.as_array_mut()) else {
        return;
    };
    for item in items {
        if !item.is_string() {
            *item = serde_json::Value::String(feedback_text(item));
        }
    }
}

fn feedback_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(values) => values
            .iter()
            .map(feedback_text)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("；"),
        serde_json::Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                let text = feedback_text(value);
                if text.is_empty() {
                    key.clone()
                } else {
                    format!("{key}：{text}")
                }
            })
            .collect::<Vec<_>>()
            .join("；"),
    }
}

fn validate_score(score: &WritingScoreV4) -> Result<(), ProviderError> {
    let values = [
        score.overall,
        score.task_response,
        score.coherence,
        score.lexical,
        score.grammar,
    ];
    if values
        .iter()
        .all(|value| value.is_finite() && (0.0..=9.0).contains(value))
    {
        Ok(())
    } else {
        Err(provider_error(
            "AI evaluation contains an invalid band score",
            false,
        ))
    }
}

fn provider_error(message: impl Into<String>, retryable: bool) -> ProviderError {
    ProviderError {
        message: message.into(),
        retryable,
    }
}

fn emit_all<E: EventSink + ?Sized>(
    sink: &E,
    events: impl IntoIterator<Item = ielts_db::EvaluationEvent>,
) {
    for event in events {
        sink.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use ielts_db::{EvaluationRunResult, EvaluationSession};
    use ielts_domain::domain::{EvaluationStage, EvaluationStatus};
    use ielts_domain::dto::WritingEvaluationV4;

    use super::*;

    #[derive(Debug, Clone)]
    struct FinishCapture {
        score: Result<WritingScoreV4, (String, bool)>,
        feedback: Option<WritingFeedbackV4>,
        review_error: Option<(String, bool)>,
    }

    struct WritingState {
        prepared: PreparedEvaluation,
        events: Vec<ielts_db::EvaluationEvent>,
        finish: Option<FinishCapture>,
        prepared_provider: Option<(String, String)>,
    }

    #[derive(Clone)]
    struct FakeWritingStore {
        state: Arc<Mutex<WritingState>>,
    }

    impl FakeWritingStore {
        fn fresh() -> Self {
            let prepared = prepared_evaluation(None);
            Self {
                state: Arc::new(Mutex::new(WritingState {
                    events: vec![event(1, EvaluationStage::Preparing)],
                    prepared,
                    finish: None,
                    prepared_provider: None,
                })),
            }
        }

        fn resumed() -> Self {
            let events = vec![
                event(1, EvaluationStage::Preparing),
                event(2, EvaluationStage::Scoring),
                event(3, EvaluationStage::Reviewing),
                event(4, EvaluationStage::Finalizing),
            ];
            let existing = completed_result(events.clone(), None, None);
            Self {
                state: Arc::new(Mutex::new(WritingState {
                    events,
                    prepared: prepared_evaluation(Some(existing)),
                    finish: None,
                    prepared_provider: None,
                })),
            }
        }
    }

    impl WritingEvaluationStore for FakeWritingStore {
        fn prepare(
            &self,
            _command: &StartEvaluationCommand,
            provider_id: &str,
            model: &str,
        ) -> Result<PreparedEvaluation, ApplicationError> {
            self.state.lock().unwrap().prepared_provider =
                Some((provider_id.to_string(), model.to_string()));
            Ok(self.state.lock().unwrap().prepared.clone())
        }

        fn list_events(
            &self,
            _evaluation_id: &str,
            after_sequence: u32,
        ) -> Result<Vec<ielts_db::EvaluationEvent>, ApplicationError> {
            Ok(self
                .state
                .lock()
                .unwrap()
                .events
                .iter()
                .filter(|event| event.sequence > after_sequence)
                .cloned()
                .collect())
        }

        fn finish(
            &self,
            prepared: &PreparedEvaluation,
            score: Result<WritingScoreV4, ProviderError>,
            feedback: Option<WritingFeedbackV4>,
            review_error: Option<ProviderError>,
        ) -> Result<EvaluationRunResult, ApplicationError> {
            let score_capture = score
                .as_ref()
                .map(Clone::clone)
                .map_err(|error| (error.message.clone(), error.retryable));
            let review_capture = review_error
                .as_ref()
                .map(|error| (error.message.clone(), error.retryable));
            let result_score = score.ok();
            let result = completed_result(
                vec![
                    event(1, EvaluationStage::Preparing),
                    event(2, EvaluationStage::Scoring),
                    event(3, EvaluationStage::Reviewing),
                    event(4, EvaluationStage::Finalizing),
                ],
                result_score,
                feedback.clone(),
            );
            assert_eq!(prepared.evaluation_id, "evaluation-1");
            self.state.lock().unwrap().finish = Some(FinishCapture {
                score: score_capture,
                feedback,
                review_error: review_capture,
            });
            Ok(result)
        }

        fn request_cancel(&self, evaluation_id: &str) -> Result<bool, ApplicationError> {
            Ok(evaluation_id == "evaluation-1")
        }
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<ielts_db::EvaluationEvent>>);

    impl EventSink for RecordingSink {
        fn emit(&self, event: ielts_db::EvaluationEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[derive(Clone)]
    struct FakeModel {
        response: Result<String, ModelError>,
        store_state: Option<Arc<Mutex<WritingState>>>,
    }

    #[async_trait]
    impl LanguageModel for FakeModel {
        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<crate::CompletionResponse, ModelError> {
            if let Some(state) = &self.store_state {
                assert!(
                    state.try_lock().is_ok(),
                    "store lock was held during model I/O"
                );
            }
            self.response
                .clone()
                .map(|content| crate::CompletionResponse {
                    content,
                    model: "fake-model".into(),
                    latency_ms: 1,
                    usage: None,
                    provider_request_id: None,
                })
        }
    }

    #[test]
    fn replays_existing_evaluation_without_starting_another_run() {
        let store = FakeWritingStore::resumed();
        let sink = RecordingSink::default();
        let outcome = WritingEvaluationService::start(
            &store,
            &start_command(),
            "openai-compatible",
            "fake-model",
            &sink,
        )
        .unwrap();

        assert!(outcome.pending.is_none());
        assert_eq!(outcome.handle.evaluation_id, "evaluation-1");
        let sequences = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![1, 2, 3, 4]);
    }

    #[test]
    fn prepare_keeps_the_selected_provider_snapshot() {
        let store = FakeWritingStore::fresh();
        let sink = RecordingSink::default();

        WritingEvaluationService::start(
            &store,
            &start_command(),
            "openai-compatible",
            "gpt-snapshot",
            &sink,
        )
        .unwrap();

        let snapshot = store.state.lock().unwrap().prepared_provider.clone();
        assert_eq!(
            snapshot,
            Some(("openai-compatible".to_string(), "gpt-snapshot".to_string()))
        );
    }

    #[tokio::test]
    async fn releases_store_lock_during_model_io_and_preserves_event_order() {
        let store = FakeWritingStore::fresh();
        let sink = RecordingSink::default();
        let model = FakeModel {
            response: Ok(valid_evaluation_json()),
            store_state: Some(store.state.clone()),
        };

        WritingEvaluationService::execute(
            &store,
            prepared_evaluation(None),
            EvaluationBackend::Language(model),
            &sink,
        )
        .await
        .unwrap();

        let sequences = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![2, 3, 4]);
        assert!(store
            .state
            .lock()
            .unwrap()
            .finish
            .as_ref()
            .unwrap()
            .score
            .is_ok());
    }

    #[tokio::test]
    async fn keeps_score_when_feedback_is_invalid() {
        let store = FakeWritingStore::fresh();
        let model = FakeModel {
            response: Ok(valid_score_with_invalid_feedback()),
            store_state: None,
        };

        WritingEvaluationService::execute(
            &store,
            prepared_evaluation(None),
            EvaluationBackend::Language(model),
            &RecordingSink::default(),
        )
        .await
        .unwrap();

        let state = store.state.lock().unwrap();
        let finish = state.finish.as_ref().unwrap();
        assert!(finish.score.is_ok());
        assert!(finish.feedback.is_none());
        assert!(finish.review_error.is_some());
        assert!(finish.review_error.as_ref().unwrap().1);
    }

    #[tokio::test]
    async fn persists_model_and_json_failures() {
        for response in [
            Err(ModelError::new("request timed out", true)),
            Ok("not-json".to_string()),
        ] {
            let store = FakeWritingStore::fresh();
            let model = FakeModel {
                response,
                store_state: None,
            };
            WritingEvaluationService::execute(
                &store,
                prepared_evaluation(None),
                EvaluationBackend::Language(model),
                &RecordingSink::default(),
            )
            .await
            .unwrap();
            assert!(store
                .state
                .lock()
                .unwrap()
                .finish
                .as_ref()
                .unwrap()
                .score
                .is_err());
        }
    }

    #[tokio::test]
    async fn persists_unavailable_backend_as_provider_failure() {
        let store = FakeWritingStore::fresh();
        WritingEvaluationService::execute(
            &store,
            prepared_evaluation(None),
            EvaluationBackend::<FakeModel>::Unavailable(ModelError::new(
                "AI is not configured",
                false,
            )),
            &RecordingSink::default(),
        )
        .await
        .unwrap();

        let state = store.state.lock().unwrap();
        let error = state.finish.as_ref().unwrap().score.as_ref().unwrap_err();
        assert_eq!(error.0, "AI is not configured");
        assert!(!error.1);
    }

    #[test]
    fn parses_fenced_json() {
        let output = parse_output("```json\n{\"score\":{\"overall\":7.0,\"taskResponse\":7.0,\"coherence\":6.5,\"lexical\":7.0,\"grammar\":6.5},\"feedback\":{\"overall\":\"Clear\",\"plan\":[],\"paragraphs\":[],\"sentences\":[],\"rewrites\":[]}}\n```").unwrap();
        assert_eq!(output.score.overall, 7.0);
        assert!(output.feedback.is_ok());
    }

    #[test]
    fn normalizes_provider_wrapped_feedback_strings() {
        let output = parse_output(
            r#"{
              "score":{"overall":7.0,"taskResponse":7.0,"coherence":6.5,"lexical":7.0,"grammar":6.5},
              "feedback":{
                "overall":{"TR":"观点清楚","CC":"衔接自然"},
                "plan":[{"step":"练习具体例证"}],
                "paragraphs":[{"paragraphIndex":1,"summary":{"strength":"立场明确"},"issues":[{"issue":"论证可更具体"}]}],
                "sentences":[{"sentence":{"original":"This is good."},"correction":{"better":"This is beneficial."},"kind":"lexical"}],
                "rewrites":[{"example":"This is beneficial."}]
              }
            }"#,
        )
        .unwrap();

        let feedback = output.feedback.unwrap();
        assert_eq!(
            feedback.overall.as_deref(),
            Some("CC：衔接自然；TR：观点清楚")
        );
        assert_eq!(feedback.plan, vec!["step：练习具体例证"]);
        assert_eq!(
            feedback.paragraphs[0].summary.as_deref(),
            Some("strength：立场明确")
        );
        assert_eq!(feedback.sentences[0].sentence, "original：This is good.");
        assert_eq!(
            feedback.sentences[0].correction.as_deref(),
            Some("better：This is beneficial.")
        );
    }

    #[test]
    fn rejects_invalid_scores_and_malformed_json() {
        assert!(parse_output("not-json").is_err());
        assert!(parse_output("{\"score\":{\"overall\":10.0,\"taskResponse\":7.0,\"coherence\":7.0,\"lexical\":7.0,\"grammar\":7.0},\"feedback\":{\"plan\":[],\"paragraphs\":[],\"sentences\":[],\"rewrites\":[]}}").is_err());
    }

    #[test]
    fn keeps_valid_score_when_review_payload_is_invalid() {
        let output = parse_output("{\"score\":{\"overall\":7.0,\"taskResponse\":7.0,\"coherence\":6.5,\"lexical\":7.0,\"grammar\":6.5},\"feedback\":false}").unwrap();
        assert_eq!(output.score.overall, 7.0);
        assert!(output.feedback.is_err());
    }

    fn start_command() -> StartEvaluationCommand {
        StartEvaluationCommand {
            attempt_id: "attempt-1".into(),
            idempotency_key: "idempotency-1".into(),
            task_type: Some("task2".into()),
            retry_of: None,
        }
    }

    fn prepared_evaluation(existing: Option<EvaluationRunResult>) -> PreparedEvaluation {
        PreparedEvaluation {
            evaluation_id: "evaluation-1".into(),
            session_id: "session-1".into(),
            essay: "A sufficiently detailed IELTS essay.".into(),
            prompt: Some("Discuss both views.".into()),
            task_type: None,
            system_prompt: "Return the required JSON schema.".into(),
            temperature: 0.2,
            prompt_id: None,
            prompt_version: "prompt-v1".into(),
            handle: ielts_db::EvaluationHandle {
                attempt_id: "attempt-1".into(),
                session_id: "session-1".into(),
                evaluation_id: "evaluation-1".into(),
                status: EvaluationStatus::Queued,
                stage: EvaluationStage::Preparing,
                retry_of: None,
                sequence: 1,
            },
            existing,
        }
    }

    fn completed_result(
        events: Vec<ielts_db::EvaluationEvent>,
        score: Option<WritingScoreV4>,
        feedback: Option<WritingFeedbackV4>,
    ) -> EvaluationRunResult {
        EvaluationRunResult {
            session: EvaluationSession {
                id: "session-1".into(),
                attempt_id: "attempt-1".into(),
                evaluation_id: "evaluation-1".into(),
                status: EvaluationStatus::Completed,
                stage: EvaluationStage::Finalizing,
                revision: 1,
                sequence: 4,
                retry_of: None,
                cancel_requested: false,
                provider_id: Some("openai-compatible".into()),
                model: Some("fake-model".into()),
                started_at: "2026-08-07T00:00:00Z".into(),
                updated_at: "2026-08-07T00:00:01Z".into(),
                completed_at: Some("2026-08-07T00:00:01Z".into()),
            },
            evaluation: WritingEvaluationV4 {
                schema_version: WritingEvaluationV4::SCHEMA_VERSION,
                id: "evaluation-1".into(),
                status: EvaluationStatus::Completed,
                stage: EvaluationStage::Finalizing,
                task_type: None,
                score,
                diagnosis: None,
                feedback,
                degradation: None,
                error: None,
            },
            events,
        }
    }

    fn event(sequence: u32, stage: EvaluationStage) -> ielts_db::EvaluationEvent {
        ielts_db::EvaluationEvent {
            evaluation_id: "evaluation-1".into(),
            sequence,
            revision: 1,
            event_type: "stage".into(),
            stage: Some(stage),
            payload: serde_json::json!({ "sequence": sequence }),
            created_at: "2026-08-07T00:00:00Z".into(),
        }
    }

    fn valid_evaluation_json() -> String {
        r#"{"score":{"overall":7.0,"taskResponse":7.0,"coherence":6.5,"lexical":7.0,"grammar":6.5},"feedback":{"overall":"Clear","plan":[],"paragraphs":[],"sentences":[],"rewrites":[]}}"#.into()
    }

    fn valid_score_with_invalid_feedback() -> String {
        r#"{"score":{"overall":7.0,"taskResponse":7.0,"coherence":6.5,"lexical":7.0,"grammar":6.5},"feedback":false}"#.into()
    }
}
