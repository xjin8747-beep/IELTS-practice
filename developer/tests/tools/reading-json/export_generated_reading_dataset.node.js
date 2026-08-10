#!/usr/bin/env node

import fs from 'fs';
import path from 'path';
import vm from 'vm';
import crypto from 'crypto';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, '../../../..');
const GENERATED_ROOT = path.join(REPO_ROOT, 'assets', 'generated', 'reading-exams');
const EXPLANATION_ROOT = path.join(REPO_ROOT, 'assets', 'generated', 'reading-explanations');
const MANIFEST_PATH = path.join(GENERATED_ROOT, 'manifest.js');

function fail(message, code = 1) {
  process.stderr.write(`${message}\n`);
  process.exit(code);
}

function readText(filePath) {
  return fs.readFileSync(filePath, 'utf8');
}

function parseArgs(argv) {
  const args = { examId: '', list: false, outputDir: '' };
  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === '--list') {
      args.list = true;
      continue;
    }
    if (token === '--exam-id') {
      args.examId = (argv[i + 1] || '').trim();
      i += 1;
      continue;
    }
    if (token === '--output-dir') {
      args.outputDir = (argv[i + 1] || '').trim();
      i += 1;
      continue;
    }
  }
  return args;
}

function stableJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function sha256(text) {
  return crypto.createHash('sha256').update(text, 'utf8').digest('hex');
}

function canonicalFrequency(examId, fallback) {
  const match = String(examId || '').match(/^p[123]-(high|medium|low)-/i);
  return match ? match[1].toLowerCase() : (fallback || null);
}

function mediaMimeType(fileName) {
  const extension = path.extname(fileName).toLowerCase();
  if (extension === '.png') return 'image/png';
  if (extension === '.jpg' || extension === '.jpeg') return 'image/jpeg';
  if (extension === '.webp') return 'image/webp';
  if (extension === '.avif') return 'image/avif';
  if (extension === '.svg') return 'image/svg+xml';
  return 'application/octet-stream';
}

function embedLocalMedia(value) {
  if (typeof value === 'string') {
    return value.replace(/(?:\.\/)?media\/([^"'<>\s)]+)/g, (reference, fileName) => {
      const mediaPath = path.join(GENERATED_ROOT, 'media', fileName);
      if (!fs.existsSync(mediaPath)) return reference;
      const encoded = fs.readFileSync(mediaPath).toString('base64');
      return `data:${mediaMimeType(fileName)};base64,${encoded}`;
    });
  }
  if (Array.isArray(value)) return value.map(embedLocalMedia);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, embedLocalMedia(item)]));
  }
  return value;
}

function exportResourcePack(context, registry, explanationRegistry, manifest, outputDir) {
  const resolvedOutput = path.resolve(REPO_ROOT, outputDir);
  const payloadDir = path.join(resolvedOutput, 'payloads');
  fs.mkdirSync(payloadDir, { recursive: true });

  const entries = buildEntryList(manifest).map((entry) => {
    const dataset = loadDataset(context, registry, manifest[entry.examId] || entry);
    const reviewExplanations = loadExplanation(context, explanationRegistry, entry.examId);
    const payload = embedLocalMedia({
      ...dataset,
      examId: dataset.examId || entry.examId,
      ...(reviewExplanations ? { reviewExplanations } : {})
    });
    const text = stableJson(payload);
    const file = `payloads/${entry.examId}.json`;
    fs.writeFileSync(path.join(resolvedOutput, file), text, 'utf8');
    return {
      examId: entry.examId,
      file,
      title: entry.title || payload.meta?.title || entry.examId,
      category: entry.category || payload.meta?.category || null,
      difficulty: payload.meta?.difficulty || null,
      frequency: canonicalFrequency(entry.examId, payload.meta?.frequency),
      sha256: sha256(text)
    };
  });

  const packManifest = {
    schemaVersion: 1,
    packId: 'ielts-reading-builtin-v1',
    assetCount: entries.length,
    entries
  };
  fs.writeFileSync(path.join(resolvedOutput, 'manifest.json'), stableJson(packManifest), 'utf8');
  process.stdout.write(`${JSON.stringify({ outputDir: resolvedOutput, assetCount: entries.length })}\n`);
}

function createRegistry() {
  const store = new Map();
  return {
    register(id, payload) {
      store.set(id, payload);
    },
    get(id) {
      return store.get(id) || null;
    },
    has(id) {
      return store.has(id);
    }
  };
}

function createContext() {
  const registry = createRegistry();
  const explanationRegistry = createRegistry();
  const context = {
    console,
    setTimeout,
    clearTimeout
  };
  context.globalThis = context;
  context.window = context;
  context.self = context;
  context.__READING_EXAM_DATA__ = registry;
  context.__READING_EXPLANATION_DATA__ = explanationRegistry;
  vm.createContext(context);
  return { context, registry, explanationRegistry };
}

function loadExplanation(context, explanationRegistry, examId) {
  const explanationPath = path.join(EXPLANATION_ROOT, `${examId}.js`);
  if (!fs.existsSync(explanationPath)) return null;
  if (!explanationRegistry.has(examId)) {
    vm.runInContext(readText(explanationPath), context, { filename: explanationPath });
  }
  return explanationRegistry.get(examId);
}

function loadManifest(context) {
  vm.runInContext(readText(MANIFEST_PATH), context, { filename: MANIFEST_PATH });
  const manifest = context.__READING_EXAM_MANIFEST__;
  if (!manifest || typeof manifest !== 'object') {
    fail('reading_manifest_missing_or_invalid');
  }
  return manifest;
}

function loadDataset(context, registry, manifestEntry) {
  if (!manifestEntry || !manifestEntry.script || !manifestEntry.dataKey) {
    fail('manifest_entry_invalid');
  }
  const scriptPath = path.join(GENERATED_ROOT, String(manifestEntry.script).replace(/^\.\//, ''));
  if (!fs.existsSync(scriptPath)) {
    fail(`reading_dataset_script_missing:${manifestEntry.script}`);
  }
  if (!registry.has(manifestEntry.dataKey)) {
    vm.runInContext(readText(scriptPath), context, { filename: scriptPath });
  }
  const dataset = registry.get(manifestEntry.dataKey);
  if (!dataset || typeof dataset !== 'object') {
    fail(`reading_dataset_missing:${manifestEntry.dataKey}`);
  }
  return dataset;
}

function buildEntryList(manifest) {
  return Object.values(manifest)
    .map((entry) => ({
      examId: entry.examId,
      dataKey: entry.dataKey,
      script: entry.script,
      title: entry.title || '',
      category: entry.category || ''
    }))
    .filter((entry) => entry.examId && entry.script && entry.dataKey)
    .sort((left, right) => String(left.examId).localeCompare(String(right.examId), 'en'));
}

function pickManifestEntry(manifest, examId) {
  if (!examId) return null;
  if (manifest[examId]) {
    return manifest[examId];
  }
  for (const entry of Object.values(manifest)) {
    if (entry && (entry.examId === examId || entry.dataKey === examId)) {
      return entry;
    }
  }
  return null;
}

function main() {
  if (!fs.existsSync(MANIFEST_PATH)) {
    fail('reading_manifest_not_found');
  }

  const args = parseArgs(process.argv);
  const { context, registry, explanationRegistry } = createContext();
  const manifest = loadManifest(context);

  if (args.outputDir) {
    exportResourcePack(context, registry, explanationRegistry, manifest, args.outputDir);
    return;
  }

  if (args.list) {
    process.stdout.write(`${JSON.stringify({ entries: buildEntryList(manifest) })}\n`);
    return;
  }

  if (!args.examId) {
    fail('missing_required_arg:--exam-id');
  }

  const entry = pickManifestEntry(manifest, args.examId);
  if (!entry) {
    fail(`reading_manifest_entry_not_found:${args.examId}`);
  }

  const dataset = loadDataset(context, registry, entry);
  const payload = {
    examId: dataset.examId || entry.examId || args.examId,
    questionOrder: Array.isArray(dataset.questionOrder) ? dataset.questionOrder : [],
    answerKey: dataset.answerKey && typeof dataset.answerKey === 'object' ? dataset.answerKey : {},
    questionGroups: Array.isArray(dataset.questionGroups) ? dataset.questionGroups : [],
    questionDisplayMap: dataset.questionDisplayMap && typeof dataset.questionDisplayMap === 'object'
      ? dataset.questionDisplayMap
      : {},
    meta: dataset.meta && typeof dataset.meta === 'object' ? dataset.meta : {},
    metaQuestionIntroHtml: dataset.meta && typeof dataset.meta.questionIntroHtml === 'string'
      ? dataset.meta.questionIntroHtml
      : '',
    script: entry.script
  };

  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

main();

