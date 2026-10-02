// Real-provider integration smoke against built or installed Windows daemons.
// Example: node scripts/tests/mvp-real-model.mjs --binary-dir target/debug
//   --model-url http://127.0.0.1:11436/v1 --model llama3.2:1b
// Existing-account CLI: --cli-provider codex-cli (or grok-cli/claude-cli).
// Expected failed CLI setup: --cli-provider grok-cli --expect-connection-error 'needs sign-in'.
// The existing local model server is never started, stopped, or reconfigured.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { appendFileSync, createWriteStream, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes, randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { stopIsolatedWindowsDaemons } from './mvp-owned-processes.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
function option(name, fallback) {
  const index = args.indexOf(name);
  if (index < 0) return fallback;
  assert(args[index + 1] && !args[index + 1].startsWith('--'), `${name} requires a value`);
  return args[index + 1];
}
const binaryRoot = resolve(option('--binary-dir', join(repoRoot, 'target/debug')));
const binaryDir = existsSync(join(binaryRoot, 'hive-daemon.exe')) ? binaryRoot : join(binaryRoot, 'resources');
const cliProvider = option('--cli-provider', undefined);
assert(!cliProvider || ['codex-cli', 'grok-cli', 'claude-cli'].includes(cliProvider), 'Unsupported --cli-provider');
const expectedConnectionError = option('--expect-connection-error', undefined);
assert(expectedConnectionError === undefined || cliProvider, '--expect-connection-error requires --cli-provider');
assert(expectedConnectionError === undefined || expectedConnectionError.trim(), '--expect-connection-error must not be empty');
const modelUrl = new URL(option('--model-url', 'http://127.0.0.1:11436/v1'));
assert(modelUrl.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(modelUrl.hostname), 'Use a local loopback HTTP model URL');
assert(!modelUrl.username && !modelUrl.password && !modelUrl.search && !modelUrl.hash, 'Use a model URL without credentials or query parameters');
const modelBaseUrl = modelUrl.href.replace(/\/$/, '').replace(/\/v1$/, '');
const expectedModel = option('--model', undefined);
const requestTimeout = Number(option('--timeout-ms', '180000'));
assert(Number.isFinite(requestTimeout) && requestTimeout > 0, '--timeout-ms must be positive');
const runId = new Date().toISOString().replace(/[:.]/g, '-') + '-' + randomUUID().slice(0, 8);
const artifactRoot = resolve(option('--output-dir', join(repoRoot, 'target/manual-test/mvp-real-model')));
const runDir = join(artifactRoot, runId);
const dataDir = join(runDir, 'data');
const documentsDir = join(runDir, 'documents');
const profileDir = join(runDir, 'profile');
for (const directory of [dataDir, documentsDir, profileDir, join(runDir, 'local-app-data'), join(runDir, 'roaming-app-data')]) mkdirSync(directory, { recursive: true });
const timeline = join(runDir, 'timeline.jsonl');
const stages = [];
const transport = [];
const ownedProcesses = new Set();
const environment = { ...process.env, USERPROFILE: profileDir, LOCALAPPDATA: join(runDir, 'local-app-data'),
  APPDATA: join(runDir, 'roaming-app-data'), RUST_LOG: 'info', ABIGAIL_VAULT_RAW_KEY: randomBytes(32).toString('base64') };
if (cliProvider) {
  // Keep authentication owned by the installed CLI. Only Abigail's data,
  // Documents and logs use the fresh acceptance profile; never copy tokens.
  for (const key of ['USERPROFILE', 'HOME', 'APPDATA', 'CODEX_HOME', 'GROK_HOME']) {
    if (process.env[key] === undefined) delete environment[key];
    else environment[key] = process.env[key];
  }
}
for (const key of ['ABIGAIL_CI_MODE', 'ABIGAIL_HIVE_URL', 'ABIGAIL_DATA_DIR', 'ABIGAIL_ENTITY_DAEMON_PATH',
  'ABIGAIL_INTERNAL_BIN_DIR', 'ABIGAIL_VAULT_PASSPHRASE', 'ABIGAIL_VAULT_DATA_DIR', 'CLAUDECODE',
  'OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'GOOGLE_API_KEY', 'GEMINI_API_KEY', 'XAI_API_KEY', 'PERPLEXITY_API_KEY']) delete environment[key];
environment.ABIGAIL_ENTITY_DAEMON_PATH = join(binaryDir, 'entity-daemon.exe');
environment.ABIGAIL_DOCUMENTS_DIR = documentsDir;
let hive;
let hiveUrl;
let restartCount = 0;
let model;
let entity;
const replies = [];
let toolEvidence;

async function stage(name, action) {
  console.log(`REAL MODEL ${name}`);
  const started = Date.now();
  try {
    const result = await action();
    const record = { name, passed: true, duration_ms: Date.now() - started };
    stages.push(record); appendFileSync(timeline, JSON.stringify(record) + '\n'); return result;
  } catch (error) {
    const record = { name, passed: false, duration_ms: Date.now() - started, error: error.message };
    stages.push(record); appendFileSync(timeline, JSON.stringify(record) + '\n'); throw error;
  }
}
async function api(url, { method = 'GET', body, timeout = requestTimeout } = {}) {
  const response = await fetch(url, { method, headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(timeout) });
  const text = await response.text();
  transport.push({ method, url, status: response.status, request: body, response: text });
  assert(response.ok, `HTTP ${response.status} from ${url}: ${text}`);
  const value = JSON.parse(text);
  assert.equal(value.ok, true, `API rejected ${url}: ${text}`);
  return value.data;
}
async function until(action, message, timeout = 90000) {
  const deadline = Date.now() + timeout;
  let lastError;
  while (Date.now() < deadline) {
    try { const result = await action(); if (result) return result; } catch (error) { lastError = error; }
    await delay(300);
  }
  throw new Error(`${message}${lastError ? ': ' + lastError.message : ''}`);
}
async function freePort() {
  const server = createServer();
  await new Promise((resolvePromise, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolvePromise); });
  const port = server.address().port;
  await new Promise(resolvePromise => server.close(resolvePromise)); return port;
}
async function stopTree(child) {
  if (!child) return;
  assert(ownedProcesses.has(child), 'Refusing to stop an unowned process');
  if (process.platform === 'win32') {
    const stopped = await stopIsolatedWindowsDaemons({ binaryDir, dataDir });
    appendFileSync(timeline, JSON.stringify({ scoped_process_cleanup: stopped }) + '\n');
  } else if (child.exitCode === null && child.signalCode === null) { child.kill('SIGTERM'); }
  await until(() => child.exitCode !== null || child.signalCode !== null, `Owned Hive ${child.pid} did not exit`, 10000);
  ownedProcesses.delete(child);
}
async function startHive() {
  const executable = join(binaryDir, 'hive-daemon.exe');
  assert(existsSync(executable), `Missing built/installed daemon ${executable}`);
  assert(existsSync(environment.ABIGAIL_ENTITY_DAEMON_PATH), `Missing built/installed Entity daemon ${environment.ABIGAIL_ENTITY_DAEMON_PATH}`);
  const port = await freePort(); hiveUrl = `http://127.0.0.1:${port}`;
  const child = spawn(executable, ['--port', String(port), '--data-dir', dataDir], { cwd: runDir, env: environment, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.pipe(createWriteStream(join(runDir, `hive-${restartCount}.stdout.txt`)));
  child.stderr.pipe(createWriteStream(join(runDir, `hive-${restartCount}.stderr.txt`)));
  child.on('error', error => appendFileSync(timeline, JSON.stringify({ spawn_error: error.message }) + '\n'));
  ownedProcesses.add(child); hive = child; restartCount++;
  appendFileSync(timeline, JSON.stringify({ owned_hive_pid: child.pid, url: hiveUrl }) + '\n');
  await until(async () => {
    if (child.exitCode !== null) throw new Error(`Hive exited with code ${child.exitCode}`);
    const response = await fetch(`${hiveUrl}/health`, { signal: AbortSignal.timeout(2000) });
    await response.text(); return response.ok;
  }, 'Hive did not become healthy');
  return until(async () => {
    const status = await api(`${hiveUrl}/v1/status`, { timeout: 5000 });
    return status.helper?.running && status.helper.local_url ? status : null;
  }, 'Hive helper did not become available');
}
async function openEntity() {
  const opened = await api(`${hiveUrl}/v1/entities/${entity.id}/open`, { method: 'POST', body: {} });
  entity.url = opened.local_url;
  const status = await api(`${entity.url}/v1/status`);
  assert.equal(status.entity_id, entity.id); assert.equal(status.name, entity.name); assert.equal(status.birth_complete, true);
  const config = await api(`${hiveUrl}/v1/entities/${entity.id}/provider-config`);
  if (cliProvider) {
    assert.equal(config.ego_provider_name, cliProvider, 'Entity did not inherit selected CLI provider');
    assert(!config.local_llm_base_url, 'Fresh CLI Entity unexpectedly uses a local fallback');
  } else {
    assert.equal(config.local_llm_base_url, modelBaseUrl);
    assert(!config.ego_provider_name, 'Fresh local-model Entity unexpectedly uses a cloud/CLI provider');
  }
}
function assertReply(response, label) {
  assert.equal(typeof response.reply, 'string', `${label} reply is missing`);
  assert(response.reply.trim().length > 0, `${label} reply is empty`);
  assert.equal(response.session_id, entity.session_id);
  const trace = response.execution_trace;
  assert(trace?.steps?.length, `${label} lacks pipeline execution attribution`);
  assert.equal(trace.steps[trace.final_step_index]?.result, 'success', `${label} lacks successful provider execution`);
  assert.equal(trace.steps[trace.final_step_index]?.provider_label, cliProvider ?? 'id(local_http)', `${label} was not served by the selected real provider`);
  assert.equal(trace.fallback_occurred, false, `${label} unexpectedly used a provider fallback`);
  replies.push({ kind: label, excerpt: response.reply.slice(0, 500), provider: response.provider,
    model_used: response.model_used, execution_trace: trace });
}
async function streamChat(message) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), requestTimeout);
  let events = ''; let tokens = ''; let tokenCount = 0; let done;
  try {
    const response = await fetch(`${entity.url}/v1/chat/stream`, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ message, session_id: entity.session_id }), signal: controller.signal });
    assert(response.ok, `Stream returned HTTP ${response.status}`);
    assert(response.headers.get('content-type')?.includes('text/event-stream'), 'Missing SSE content type');
    const reader = response.body.getReader(); const decoder = new TextDecoder();
    let pending = '';
    while (!done) {
      const chunk = await reader.read(); if (chunk.done) break;
      const text = decoder.decode(chunk.value, { stream: true }); events += text; pending += text;
      const frames = pending.split(/\r?\n\r?\n/); pending = frames.pop();
      for (const frame of frames) {
        const event = frame.match(/^event:\s*(.+)$/m)?.[1]?.trim();
        const data = frame.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).replace(/^ /, '')).join('\n');
        if (event === 'error') throw new Error(`Stream error: ${data}`);
        if (event === 'token') { tokens += data; tokenCount++; }
        if (event === 'done') { done = JSON.parse(data); break; }
      }
    }
    await reader.cancel();
    assert(done, 'Stream ended without a done event');
    assert(tokenCount > 0 && tokens.trim(), 'Stream delivered no live text tokens');
    assertReply(done, 'stream');
    replies.at(-1).token_count = tokenCount;
    return done;
  } finally {
    clearTimeout(timer); controller.abort();
    transport.push({ method: 'POST SSE', url: `${entity.url}/v1/chat/stream`, request: { message, session_id: entity.session_id }, events });
  }
}
async function history() {
  const explicit = await api(`${entity.url}/v1/chat/history?session_id=${encodeURIComponent(entity.session_id)}&limit=200`);
  const latest = await api(`${entity.url}/v1/chat/history?limit=200`);
  assert.equal(explicit.session_id, entity.session_id); assert.deepEqual(latest, explicit, 'Default window history does not restore the latest durable session');
  return explicit;
}

async function verifyExpectedConnectionFailure() {
  await stage('discover official installed CLI before negative connection validation', async () => {
    const detection = await api(`${hiveUrl}/v1/providers/detect`);
    const installed = detection.providers.find(provider => provider.provider === cliProvider);
    assert(installed?.on_path && installed.is_official, 'Official installed CLI was not discovered');
  });
  const failure = await stage('reject the expected CLI connection error without saving a provider', async () => {
    const url = `${hiveUrl}/v1/providers/hive-default`;
    const body = { provider: cliProvider };
    const response = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body), signal: AbortSignal.timeout(requestTimeout) });
    const text = await response.text();
    transport.push({ method: 'POST', url, status: response.status, request: body, response: text });
    assert(response.ok, `HTTP ${response.status} from ${url}: ${text}`);
    const envelope = JSON.parse(text);
    assert.equal(envelope.ok, false, 'Expected failed CLI setup unexpectedly succeeded');
    assert.equal(typeof envelope.error, 'string', 'Failed CLI setup did not return an error message');
    assert(envelope.error.toLowerCase().includes(expectedConnectionError.toLowerCase()),
      `Expected connection error containing ${JSON.stringify(expectedConnectionError)}, received ${JSON.stringify(envelope.error)}`);
    return envelope.error;
  });
  await stage('keep Hive unconfigured after the rejected CLI connection', async () => {
    const status = await api(`${hiveUrl}/v1/status`);
    assert.equal(status.any_provider_configured, false, 'Rejected connection incorrectly marked Hive configured');
    assert.equal(status.ready_state, 'needs_provider');
    assert.equal(status.setup_complete, false);
    const hiveEntity = status.entities.find(candidate => candidate.is_hive);
    assert(hiveEntity, 'Hive identity is missing');
    const providerConfig = await api(`${hiveUrl}/v1/entities/${hiveEntity.id}/provider-config`);
    assert(!providerConfig.ego_provider_name, 'Rejected connection saved a resolved Hive default');
    assert(!providerConfig.ego_model, 'Rejected connection saved a model');
    assert(!providerConfig.local_llm_base_url, 'Rejected connection unexpectedly saved a local model');
    const savedConfig = JSON.parse(readFileSync(join(dataDir, 'identities', hiveEntity.id, 'config.json'), 'utf8'));
    assert(!savedConfig.active_provider_preference, 'Rejected connection persisted a Hive default');
    assert(!savedConfig.ego_model, 'Rejected connection persisted a model');
    assert(!savedConfig.local_llm_base_url, 'Rejected connection persisted a local model');
  });
  return { provider: cliProvider, expected_error_substring: expectedConnectionError, error: failure,
    passed: true, default_saved: false, ready_state: 'needs_provider', any_provider_configured: false };
}

async function main() {
  console.log(expectedConnectionError === undefined
    ? 'Real model integration smoke; synthetic household prompts only, no reply-quality scoring.'
    : 'Negative CLI connection validation; no successful model inference is claimed.');
  await stage('start isolated Hive and helper from built/installed binaries', startHive);
  if (expectedConnectionError !== undefined) {
    const validation = await verifyExpectedConnectionFailure();
    return { passed: true, negative_connection_validation: validation };
  }
  await stage(cliProvider ? 'discover installed CLI and validate actual existing-account inference in Hive' : 'discover and test the actual local model through Hive setup', async () => {
    if (cliProvider) {
      const detection = await api(`${hiveUrl}/v1/providers/detect`);
      const installed = detection.providers.find(provider => provider.provider === cliProvider);
      assert(installed?.on_path && installed.is_official, 'Official installed CLI was not discovered');
      const provider = await api(`${hiveUrl}/v1/providers/hive-default`, { method: 'POST', body: { provider: cliProvider } });
      assert.equal(provider.provider, cliProvider);
      model = provider.model;
      return;
    }
    const provider = await api(`${hiveUrl}/v1/providers/local`, { method: 'POST', body: { base_url: modelUrl.href } });
    assert.equal(provider.base_url, modelBaseUrl); assert(provider.model?.trim(), 'Hive did not discover a model');
    model = provider.model; if (expectedModel) assert.equal(model, expectedModel);
  });
  await stage('create named Entity and signed quickstart birth with practical purpose', async () => {
    const name = 'MVP Real Model Ada';
    const purpose = 'Help this synthetic household organize groceries and simple routines with warm, concise suggestions.';
    const created = await api(`${hiveUrl}/v1/entities`, { method: 'POST', body: { name } });
    entity = { id: created.id, name, purpose, session_id: `entity-${created.id}` };
    const birth = await api(`${hiveUrl}/v1/entities/${entity.id}/birth`, { method: 'POST', body: { path: 'quickstart', choices: [], purpose } });
    assert.equal(birth.certificate.entity_id, entity.id); assert(birth.certificate.signature, 'Birth certificate is unsigned');
    assert(existsSync(join(documentsDir, 'Abigail', name)), 'Missing isolated Entity Documents folder');
    assert(readFileSync(join(dataDir, 'identities', entity.id, 'docs', 'soul.md'), 'utf8').includes(purpose), 'Birth did not persist the supplied purpose in its identity soul document');
  });
  await stage('open Entity while Hive helper remains available', async () => {
    await openEntity();
    const status = await api(`${hiveUrl}/v1/status`); assert.equal(status.helper.running, true);
  });
  const verificationWord = `familylist${randomBytes(6).toString('hex')}`;
  const plainMessage = 'Suggest two simple ways to organize a shared family grocery list. Use two short bullet points; no external tools are needed.'
    + (cliProvider ? ` Also remember this verification word for my next question: ${verificationWord}.` : '');
  const streamMessage = cliProvider
    ? 'What verification word did I give you in my previous message? Include that exact word, followed by one short five-minute living-room tidying suggestion. No external tools are needed.'
    : 'Suggest one simple five-minute routine for tidying a shared living room. Answer in one short sentence; no external tools are needed.';
  const plain = await stage('complete a practical synthetic message through the actual pipeline', async () => {
    const response = await api(`${entity.url}/v1/chat`, { method: 'POST', body: { message: plainMessage, session_id: entity.session_id } });
    assertReply(response, 'plain'); return response;
  });
  const streamed = await stage('deliver live tokens and done for a practical synthetic SSE message', () => streamChat(streamMessage));
  if (cliProvider) await stage('replay Entity conversation context through the installed-account adapter', async () => {
    assert(streamed.reply.toLowerCase().includes(verificationWord), 'The installed-account provider did not receive the previous Entity turn');
  });
  await stage(cliProvider ? 'selected CLI serves Entity turns while native tools remain disabled' : 'real model accepts the Entity pipeline with tool definitions', async () => {
    if (cliProvider) {
      // Successful attributed turns above enter the same Entity pipeline that
      // supplies Abigail's skills. The CLI adapters use no native tool authority.
      toolEvidence = { provider: cliProvider, plain_and_stream_succeeded: true,
        native_tool_isolation_validation: 'covered separately by adapter protocol tests' };
      return;
    }
    const logPath = join(environment.LOCALAPPDATA, 'Abigail', 'logs', `entity-daemon-${entity.id}.log`);
    const matches = readFileSync(logPath, 'utf8').split(/\r?\n/).map(line => {
      const match = line.match(/LocalHttp::(complete|stream) base_url=([^,]+), model=([^,]+), messages=(\d+), tools=(\d+)/);
      return match && { method: match[1], base_url: match[2], model: match[3], messages: Number(match[4]), tools: Number(match[5]) };
    }).filter(Boolean);
    toolEvidence = matches.filter(call => call.base_url === modelBaseUrl && call.model === model && call.tools > 0);
    for (const method of ['complete', 'stream']) assert(toolEvidence.some(call => call.method === method), `No ${method} call to the real model with tool definitions was recorded`);
  });
  const expectedHistory = await stage('save both real replies in the durable default Entity session', async () => {
    const expected = { session_id: entity.session_id, messages: [
      { role: 'user', content: plainMessage }, { role: 'assistant', content: plain.reply },
      { role: 'user', content: streamMessage }, { role: 'assistant', content: streamed.reply },
    ] };
    assert.deepEqual(await history(), expected); return expected;
  });
  await stage('preserve exact transcript after Entity close and reopen', async () => {
    await api(`${hiveUrl}/v1/entities/${entity.id}/close`, { method: 'POST', body: {} });
    await openEntity(); assert.deepEqual(await history(), expectedHistory);
  });
  await stage('preserve identity, selected provider and transcript across whole Hive restart', async () => {
    await stopTree(hive); await delay(500);
    const restarted = await startHive();
    assert(restarted.entities.some(candidate => candidate.id === entity.id && candidate.name === entity.name && candidate.birth_complete));
    await openEntity(); assert.deepEqual(await history(), expectedHistory);
    assert(existsSync(join(dataDir, 'memory.db', 'wal')), 'The durable database WAL is missing from the isolated data root');
  });
  return { passed: true };
}

let result;
try {
  result = await main();
} catch (error) {
  result = { passed: false, error: error.stack };
  console.error(error.stack); process.exitCode = 1;
} finally {
  for (const child of [...ownedProcesses]) {
    try { await stopTree(child); } catch (error) { result.cleanup_error = error.message; result.passed = false; process.exitCode = 1; }
  }
  Object.assign(result, { kind: expectedConnectionError === undefined ? 'real model integration smoke' : 'negative CLI connection validation',
    real_model_validation: expectedConnectionError === undefined, model_quality_evaluation: false,
    local_only: !cliProvider, existing_account_cli: cliProvider, api_key_injected: false,
    model_url: cliProvider ? undefined : `${modelBaseUrl}/v1`, model, binary_dir: binaryDir, data_dir: dataDir,
    documents_dir: documentsDir, profile_dir: profileDir, entity, response_excerpts: replies, tool_compatible_pipeline_calls: toolEvidence, stages });
  writeFileSync(join(runDir, 'http-traces.json'), JSON.stringify(transport, null, 2));
  writeFileSync(join(runDir, 'result.json'), JSON.stringify(result, null, 2));
  writeFileSync(join(artifactRoot, 'latest.json'), JSON.stringify({ run_dir: runDir, passed: result.passed }, null, 2));
  console.log(`${expectedConnectionError === undefined ? 'Real model' : 'Negative CLI connection'} result: ${result.passed ? 'PASS' : 'FAIL'}; ${join(runDir, 'result.json')}`);
}
