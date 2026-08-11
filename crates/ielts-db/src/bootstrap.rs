//! One-time defaults for the combined IELTS desktop product.
//!
//! These are deliberately separate from schema migrations: the database crate
//! remains neutral in tests and other hosts, while the desktop application can
//! opt into an IELTS-ready prompt bank and DeepSeek defaults exactly once.

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::{DbResult, NS_AI};

const MARKER: &str = "combined_product_defaults.2026_08_sentence_feedback";

pub const TASK1_EXAMINER_PROMPT: &str = r#"You are a strict, evidence-based IELTS Academic Writing Task 1 examiner. Assess only the submitted task and response. Apply the four IELTS criteria independently: Task Achievement, Coherence and Cohesion, Lexical Resource, and Grammatical Range and Accuracy. In the JSON score object, taskResponse represents Task Achievement.

For Task Achievement, verify that the response addresses the actual visual or process described in the task, contains a clear overview, selects the most important features, makes accurate comparisons, and supports statements with the supplied data. Penalize missing or inaccurate overview statements, invented figures, exhaustive detail without selection, and responses below 150 words according to their real impact. Do not reward memorised introductions or advanced vocabulary by themselves. For the other criteria, judge progression, paragraphing, referencing and linking, range and precision of vocabulary, collocation and spelling, sentence variety, punctuation and grammar accuracy. Base every criticism on evidence in the response. Use only whole or half bands from 0 to 9 and keep the overall band consistent with the four criteria.

Return valid JSON only, without markdown or text outside JSON. Use exactly this structure:
{"score":{"overall":0,"taskResponse":0,"coherence":0,"lexical":0,"grammar":0},"feedback":{"overall":"","plan":[],"paragraphs":[{"paragraphIndex":1,"summary":"","issues":[]}],"sentences":[{"sentence":"","correction":"","kind":"major_grammar"}],"rewrites":[]}}

Write explanations in clear Chinese while preserving useful English examples. feedback.overall must justify every criterion band and identify the highest-priority weaknesses. feedback.plan must contain concrete practice steps. Include every essay paragraph in feedback.paragraphs. feedback.sentences must contain 3 to 6 representative sentence-level observations and must never be empty. If the response has few outright errors, select sentences that can be made more precise, concise, natural, or better linked; put the original sentence in sentence and a concrete improved version in correction. Classify kind as task, coherence, lexical, major_grammar, minor_grammar, spelling, or punctuation. feedback.rewrites must contain 2 to 4 targeted improved examples. Rewrites must preserve the original data and meaning; never invent chart values, features, or a different response."#;

pub const TASK2_EXAMINER_PROMPT: &str = r#"You are a strict, evidence-based IELTS Academic Writing Task 2 examiner. Assess only the submitted question and essay. Apply the four IELTS criteria independently: Task Response, Coherence and Cohesion, Lexical Resource, and Grammatical Range and Accuracy.

For Task Response, verify that every part of the question is answered, the position is clear and consistent, main ideas are relevant and sufficiently developed, and support is specific and logically connected. Penalize partial coverage, unclear position, memorised or irrelevant material, unsupported assertions, repetition, and responses below 250 words according to their real impact. Do not reward advanced vocabulary by itself. For Coherence and Cohesion, judge overall progression, paragraph focus, referencing and natural linking rather than connector count. For Lexical Resource, judge range, precision, collocation, word formation and spelling. For Grammatical Range and Accuracy, judge sentence variety, control, punctuation, error frequency and effect on meaning. Base every criticism on exact evidence in the essay. Use only whole or half bands from 0 to 9 and keep the overall band consistent with the four criteria.

Return valid JSON only, without markdown or text outside JSON. Use exactly this structure:
{"score":{"overall":0,"taskResponse":0,"coherence":0,"lexical":0,"grammar":0},"feedback":{"overall":"","plan":[],"paragraphs":[{"paragraphIndex":1,"summary":"","issues":[]}],"sentences":[{"sentence":"","correction":"","kind":"major_grammar"}],"rewrites":[]}}

Write explanations in clear Chinese while preserving useful English examples. feedback.overall must justify every criterion band and identify the highest-priority weaknesses. feedback.plan must contain concrete practice steps. Include every essay paragraph in feedback.paragraphs. feedback.sentences must contain 3 to 6 representative sentence-level observations and must never be empty. If the essay has few outright errors, select sentences that can be made more precise, concise, natural, or better linked; put the original sentence in sentence and a concrete improved version in correction. Classify kind as task, coherence, lexical, major_grammar, minor_grammar, spelling, or punctuation. feedback.rewrites must contain 2 to 4 targeted improved examples. Rewrites must preserve the writer position and core ideas; improve expression and development without inventing a different argument."#;

/// Install product defaults once without resurrecting anything the user later
/// edits or deletes. Existing prompt banks and explicit model settings win.
pub fn ensure_combined_product_defaults(conn: &Connection) -> DbResult<bool> {
    let already_applied: Option<String> = conn
        .query_row(
            "SELECT value FROM migration_meta WHERE key = ?1",
            params![MARKER],
            |row| row.get(0),
        )
        .optional()?;
    if already_applied.is_some() {
        return Ok(false);
    }

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let now = chrono::Utc::now().to_rfc3339();

    seed_prompt(
        &tx,
        "builtin-ielts-task1-2026-08",
        "task1",
        "ielts-task1-2026.08.1",
        TASK1_EXAMINER_PROMPT,
        &now,
    )?;
    seed_prompt(
        &tx,
        "builtin-ielts-task2-2026-08",
        "task2",
        "ielts-task2-2026.08.1",
        TASK2_EXAMINER_PROMPT,
        &now,
    )?;

    // Upgrade only the built-in August prompts. User-created prompt IDs remain
    // untouched, while existing combined-product installs gain guaranteed
    // sentence-level feedback and rewrite examples.
    tx.execute(
        "UPDATE writing_prompts
         SET version = 'ielts-task1-2026.08.1', body = ?1, updated_at = ?2
         WHERE id = 'builtin-ielts-task1-2026-08'
           AND version = 'ielts-task1-2026.08'",
        params![TASK1_EXAMINER_PROMPT, now],
    )?;
    tx.execute(
        "UPDATE writing_prompts
         SET version = 'ielts-task2-2026.08.1', body = ?1, updated_at = ?2
         WHERE id = 'builtin-ielts-task2-2026-08'
           AND version = 'ielts-task2-2026.08'",
        params![TASK2_EXAMINER_PROMPT, now],
    )?;

    for (key, value_json) in [
        ("temperature_mode", "\"custom\""),
        ("temperature_task1", "0.2"),
        ("temperature_task2", "0.2"),
    ] {
        tx.execute(
            "INSERT OR IGNORE INTO settings(namespace, key, value_json, updated_at)
             VALUES ('app', ?1, ?2, ?3)",
            params![key, value_json, now],
        )?;
    }

    // The legacy names were retired by DeepSeek in July 2026. Upgrade only
    // DeepSeek configurations that still use those names; custom V4 choices
    // and every non-DeepSeek provider remain untouched.
    tx.execute(
        "UPDATE settings
         SET value_json = json_set(
               value_json,
               '$.defaultModel', 'deepseek-v4-pro',
               '$.baseUrl', 'https://api.deepseek.com'
             ),
             updated_at = ?1
         WHERE namespace = ?2
           AND key LIKE 'config:%'
           AND json_valid(value_json)
           AND lower(COALESCE(json_extract(value_json, '$.provider'), '')) = 'deepseek'
           AND lower(COALESCE(json_extract(value_json, '$.defaultModel'), ''))
               IN ('deepseek-chat', 'deepseek-reasoner')",
        params![now, NS_AI],
    )?;
    tx.execute(
        "UPDATE settings SET value_json = '\"deepseek-v4-pro\"', updated_at = ?1
         WHERE namespace = ?2 AND key = 'model'
           AND lower(value_json) IN ('\"deepseek-chat\"', '\"deepseek-reasoner\"')",
        params![now, NS_AI],
    )?;
    tx.execute(
        "UPDATE settings SET value_json = '\"https://api.deepseek.com\"', updated_at = ?1
         WHERE namespace = ?2 AND key = 'baseUrl'
           AND value_json IN ('\"https://api.deepseek.com/v1\"', '\"https://api.deepseek.com/v1/\"')",
        params![now, NS_AI],
    )?;

    tx.execute(
        "INSERT INTO migration_meta(key, value) VALUES (?1, ?2)",
        params![MARKER, now],
    )?;
    tx.commit()?;
    Ok(true)
}

fn seed_prompt(
    tx: &Transaction<'_>,
    id: &str,
    task_type: &str,
    version: &str,
    body: &str,
    now: &str,
) -> DbResult<()> {
    tx.execute(
        "INSERT INTO writing_prompts(
           id, task_type, version, body, is_active, created_at, updated_at
         )
         SELECT ?1, ?2, ?3, ?4, 1, ?5, ?5
         WHERE NOT EXISTS (
           SELECT 1 FROM writing_prompts WHERE task_type = ?2
         )",
        params![id, task_type, version, body, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{get_setting, list_writing_prompts, migrate};
    use ielts_domain::domain::WritingTaskType;

    #[test]
    fn seeds_once_and_preserves_later_user_changes() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();

        assert!(ensure_combined_product_defaults(&conn).unwrap());
        let prompts = list_writing_prompts(&conn, None).unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(prompts.iter().all(|prompt| prompt.is_active));
        assert!(TASK1_EXAMINER_PROMPT.contains("Task Achievement"));
        assert!(TASK2_EXAMINER_PROMPT.contains("Task Response"));
        assert_eq!(
            get_setting(&conn, "app", "temperature_task1")
                .unwrap()
                .unwrap()
                .value,
            serde_json::json!(0.2)
        );

        conn.execute(
            "DELETE FROM writing_prompts WHERE task_type = 'task1'",
            [],
        )
        .unwrap();
        assert!(!ensure_combined_product_defaults(&conn).unwrap());
        assert!(list_writing_prompts(&conn, Some(WritingTaskType::Task1))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn upgrades_august_builtin_prompts_with_required_sentence_feedback() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO writing_prompts(
               id, task_type, version, body, is_active, created_at, updated_at
             ) VALUES (?1, 'task2', 'ielts-task2-2026.08', 'OLD BODY', 1, ?2, ?2)",
            params!["builtin-ielts-task2-2026-08", now],
        )
        .unwrap();

        assert!(ensure_combined_product_defaults(&conn).unwrap());
        let (version, body): (String, String) = conn
            .query_row(
                "SELECT version, body FROM writing_prompts
                 WHERE id = 'builtin-ielts-task2-2026-08'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(version, "ielts-task2-2026.08.1");
        assert!(body.contains("must contain 3 to 6"));
        assert!(body.contains("must never be empty"));
    }

    #[test]
    fn upgrades_only_retired_deepseek_model_names() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO settings(namespace,key,value_json,updated_at)
             VALUES ('ai','config:deepseek',?1,?2),('ai','config:openai',?3,?2)",
            params![
                serde_json::json!({
                    "id":"deepseek","configName":"DeepSeek","provider":"deepseek",
                    "baseUrl":"https://api.deepseek.com/v1","defaultModel":"deepseek-chat",
                    "isDefault":false,"isEnabled":true,"hasSecret":false
                }).to_string(),
                now,
                serde_json::json!({
                    "id":"openai","configName":"OpenAI","provider":"openai",
                    "baseUrl":"https://api.openai.com/v1","defaultModel":"gpt-4o-mini",
                    "isDefault":false,"isEnabled":true,"hasSecret":false
                }).to_string(),
            ],
        )
        .unwrap();

        ensure_combined_product_defaults(&conn).unwrap();
        let deepseek: String = conn
            .query_row(
                "SELECT value_json FROM settings WHERE namespace='ai' AND key='config:deepseek'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let openai: String = conn
            .query_row(
                "SELECT value_json FROM settings WHERE namespace='ai' AND key='config:openai'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(deepseek.contains("deepseek-v4-pro"));
        assert!(deepseek.contains("https://api.deepseek.com"));
        assert!(openai.contains("gpt-4o-mini"));
    }
}

