import assert from 'node:assert/strict'
import fs from 'node:fs'

const read = (file) => fs.readFileSync(file, 'utf8')
const manifest = JSON.parse(read('assets/resource-pack/reading/manifest.json'))

assert.equal(manifest.assetCount, 234, 'August main reading pack must contain 234 assets')
assert.ok(manifest.entries.every((entry) => ['high', 'medium', 'low'].includes(entry.frequency)))

let explanationCount = 0
let embeddedImageCount = 0
for (const entry of manifest.entries) {
  const rawPayload = read(`assets/resource-pack/reading/${entry.file}`)
  const payload = JSON.parse(rawPayload)
  assert.equal(payload.examId, entry.examId)
  assert.ok(payload.passage, `${entry.examId} must include its passage`)
  assert.ok(payload.answerKey && Object.keys(payload.answerKey).length, `${entry.examId} must include answers`)
  if (payload.reviewExplanations) explanationCount += 1
  embeddedImageCount += (rawPayload.match(/data:image\//g) || []).length
}
assert.equal(explanationCount, 227, 'all explanations available on main must be bundled')
assert.equal(embeddedImageCount, 3, 'all diagram images must be embedded for offline use')

const coach = read('apps/writing-vue/src/modules/practice-reading/useReadingCoach.ts')
for (const field of ['passage', 'questionGroups', 'answerKey', 'existingExplanations']) {
  assert.ok(coach.includes(field), `coach context must include ${field}`)
}

const settings = read('apps/writing-vue/src/views/SettingsPage.vue')
assert.match(settings, /provider:\s*'deepseek'/)
assert.match(settings, /default_model:\s*'deepseek-v4-pro'/)

const bootstrap = read('crates/ielts-db/src/bootstrap.rs')
for (const expected of [
  'TASK1_EXAMINER_PROMPT',
  'TASK2_EXAMINER_PROMPT',
  'deepseek-v4-pro',
  'temperature_task1',
  'temperature_task2'
]) {
  assert.ok(bootstrap.includes(expected), `combined defaults must include ${expected}`)
}

const aiRuntime = read('src-tauri/src/ai/runtime.rs')
for (const expected of ['max_tokens', 'reasoning_effort', '"enabled"', '"disabled"']) {
  assert.ok(aiRuntime.includes(expected), `DeepSeek runtime must include ${expected}`)
}

const evaluatingPage = read('apps/writing-vue/src/views/EvaluatingPage.vue')
assert.match(
  evaluatingPage,
  /import\s*\{\s*normalizeMap\s*\}\s*from\s*['"]@\/utils\/evaluation-result\.js['"]/,
  'evaluation progress must import the payload normalizer it calls'
)

const aiCommands = read('src-tauri/src/commands/ai.rs')
assert.ok(aiCommands.includes('should_promote_new_config'), 'a newly saved usable Key must become default')
assert.ok(aiCommands.includes('secret.trim()'), 'pasted API keys must be trimmed before secure storage')

const writingPolicy = read('crates/ielts-db/src/writing/eval_resolve.rs')
for (const criterion of [
  'Task Achievement',
  'Task Response',
  'Coherence and Cohesion',
  'Lexical Resource',
  'Grammatical Range and Accuracy',
  'feedback.paragraphs',
  'feedback.sentences'
]) {
  assert.ok(writingPolicy.includes(criterion), `default writing policy must contain ${criterion}`)
}

const vault = read('crates/ielts-db/src/secrets/mod.rs')
assert.ok(vault.includes('keyring::Entry'), 'API keys must use the OS credential manager')
assert.ok(vault.includes('skip_serializing'), 'legacy secret bytes must never be serialized')

const appState = read('src-tauri/src/app/state.rs')
assert.ok(appState.includes('IELTS_PRACTICE_DATA_DIR'), 'portable data root must support an override')
assert.match(appState, /install_dir\.join\("data"\)/, 'Windows study data must stay beside the app')

const readingAssets = read('crates/ielts-db/src/reading/assets.rs')
assert.ok(readingAssets.includes('resource_text_checksum'), 'Windows CRLF resource packs must validate')

console.log('combined build contract: ok')
