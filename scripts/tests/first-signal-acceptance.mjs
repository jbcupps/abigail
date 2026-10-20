// Synthetic full-daemon acceptance. Default invocation only prints the plan.
// Live execution is intentionally blocked while startup reads the OS keyring
// before the supplied test key. Review that gap before enabling a future run;
// this script does not change credential code, protection modes, or settings.
//
// node scripts/tests/first-signal-acceptance.mjs --plan
// node scripts/tests/first-signal-acceptance.mjs --run --binary-dir target/debug
//
// The local model is a deterministic HTTP fixture, not real-model validation.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import {
  appendFileSync, createReadStream, createWriteStream, existsSync,
  mkdirSync, readFileSync, statSync, writeFileSync,
} from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { startModelFixture } from './mvp-model-fixture.mjs';
import { stopIsolatedWindowsDaemons } from './mvp-owned-processes.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
function option(name, fallback) {
  const index = args.indexOf(name);
  if (index < 0) return fallback;
  assert(args[index + 1] && !args[index + 1].startsWith('--'), `${name} needs a value`);
  return args[index + 1];
}
for (let index = 0; index < args.length; index++) {
  if (['--binary-dir', '--output-dir'].includes(args[index])) { index++; continue; }
  assert(['--plan', '--run'].includes(args[index]), `Unknown option: ${args[index]}`);
}
assert(!(args.includes('--plan') && args.includes('--run')), 'Choose --plan or --run');
const runRequested = args.includes('--run');
const binaryDir = resolve(option('--binary-dir', join(repoRoot, 'target/debug')));
const artifactRoot = resolve(option('--output-dir', join(repoRoot, 'target/manual-test/first-signal')));
const sourceFiles = [
  'crates/abigail-core/src/vault/unlock.rs',
  'crates/abigail-identity/src/lib.rs',
  'crates/hive-daemon/src/routes.rs',
  'crates/hive-daemon/src/main.rs',
  'crates/hive-daemon/src/presence.rs',
  'crates/hive-daemon/src/presence_profile.rs',
];
const checks = [
  'Fresh lookup is absent; keyed create completes; replay returns the exact ID',
  'Four concurrent creates using one fresh key return one ID and one registry row',
  'Different name with the same key returns 409; malformed key returns 400',
  'Presence defaults, save, reload and invalid-save preservation; display label does not rename Entity',
  'Default quickstart birth and signed certificate; isolated Documents',
  'Open, synthetic nonstream chat, SSE, persisted history, close and reopen',
  'Restart the same isolated Hive data; replay exact keys, presence and history unchanged',
];

function inspectIsolation() {
  const file = sourceFiles[0];
  const source = readFileSync(join(repoRoot, file), 'utf8');
  const start = source.indexOf('impl UnlockProvider for HybridUnlockProvider');
  const end = source.indexOf('/// Provider that always derives', start);
  const body = source.slice(start, end);
  const keyring = body.indexOf('os_keyring_load_optional()');
  const raw = body.indexOf('std::env::var(RAW_KEK_ENV)');
  const line = offset => source.slice(0, start + offset).split('\n').length;
  // This detects the specific inspected gap; it is not a general isolation proof.
  const knownGap = start >= 0 && end > start && keyring >= 0 && raw > keyring;
  return {
    ready: false,
    reason: knownGap
      ? 'HybridUnlockProvider reads the OS credential store before ABIGAIL_VAULT_RAW_KEY. No supported no-keyring daemon startup has been established.'
      : 'Startup source changed; no-keyring isolation must be reviewed before this draft harness is enabled.',
    source: file,
    ...(knownGap ? { os_keyring_line: line(keyring), raw_test_key_line: line(raw) } : {}),
  };
}

const isolation = inspectIsolation();
const plan = {
  kind: 'synthetic first-signal daemon acceptance plan',
  executed: false,
  real_model_validation: false,
  binary_dir: binaryDir,
  output_root: artifactRoot,
  checks,
  intentionally_uncovered: [
    'Reserved and unavailable recovery states need controlled in-process failure tests; this HTTP harness does not modify registry files.',
    'Actual providers, microphone, video assets, visual UI, OS keyring, user profile and user Documents.',
  ],
  isolation,
};
if (!runRequested) {
  console.log(JSON.stringify(plan, null, 2));
} else if (!isolation.ready) {
  // No fixture/socket/process/filesystem mutation happens before this gate.
  console.error(JSON.stringify({ ...plan, blocked: true }, null, 2));
  process.exitCode = 2;
} else {
  await runAcceptance();
}

async function runAcceptance() {
  // Reviewed execution is Windows-only because cleanup verifies exact executable
  // paths and --data-dir for both Hive and its supervised Entity processes.
  assert.equal(process.platform, 'win32', 'This harness requires Windows scoped process cleanup');
  for (const binary of ['hive-daemon.exe', 'entity-daemon.exe']) {
    assert(existsSync(join(binaryDir, binary)), `Missing built current-source binary: ${binary}`);
  }
  const runId = `${new Date().toISOString().replace(/[:.]/g, '-')}-${randomUUID().slice(0, 8)}`;
  const runDir = join(artifactRoot, runId);
  assert(!existsSync(runDir), 'Acceptance output must be a fresh directory');
  const dataDir = join(runDir, 'data');
  const documentsDir = join(runDir, 'documents');
  const profileDir = join(runDir, 'profile');
  const tempDir = join(runDir, 'temp');
  for (const path of [dataDir, documentsDir, profileDir, tempDir]) mkdirSync(path, { recursive: true });
  const timeline = join(runDir, 'timeline.jsonl');
  const stages = [];
  const transport = [];
  const ownedProcesses = new Set();
  // Do not inherit credentials, provider URLs, proxies, CLI config, or application
  // modes. This containment does not bypass the OS-keyring startup gate above.
  const environment = {};
  for (const key of ['SystemRoot', 'WINDIR', 'SystemDrive', 'COMSPEC', 'PATHEXT', 'NUMBER_OF_PROCESSORS', 'PROCESSOR_ARCHITECTURE']) {
    if (process.env[key]) environment[key] = process.env[key];
  }
  Object.assign(environment, {
    PATH: `${binaryDir};${join(process.env.SystemRoot ?? 'C:\\Windows', 'System32')}`,
    LOCALAPPDATA: join(runDir, 'local-app-data'), APPDATA: join(runDir, 'roaming-app-data'),
    USERPROFILE: profileDir, HOME: profileDir, TEMP: tempDir, TMP: tempDir,
    RUST_LOG: 'info',
    ABIGAIL_DOCUMENTS_DIR: documentsDir,
    ABIGAIL_ENTITY_DAEMON_PATH: join(binaryDir, 'entity-daemon.exe'),
    ABIGAIL_VAULT_RAW_KEY: randomBytes(32).toString('base64'),
  });
  const metadata = {
    kind: 'synthetic first-signal daemon contract acceptance',
    real_model_validation: false, run_id: runId, binary_dir: binaryDir,
    data_dir: dataDir, documents_dir: documentsDir, sources: {}, binaries: {},
    intentionally_uncovered: plan.intentionally_uncovered,
  };
  for (const file of sourceFiles) metadata.sources[file] = await hashFile(join(repoRoot, file));
  for (const file of ['hive-daemon.exe', 'entity-daemon.exe']) {
    const path = join(binaryDir, file);
    metadata.binaries[file] = { sha256: await hashFile(path), modified: statSync(path).mtime.toISOString() };
  }
  let fixture;
  let hiveUrl;
  let generation = 0;
  let result = { ...metadata, passed: false, stages };
  const entities = [];
  const log = record => appendFileSync(timeline, JSON.stringify(record) + '\n');
  const fullTimeout = AbortSignal.timeout(8 * 60_000);

  async function stage(name, action) {
    console.log(`FIRST SIGNAL ${name}`);
    const started = Date.now();
    try {
      fullTimeout.throwIfAborted();
      const value = await action();
      const record = { name, passed: true, duration_ms: Date.now() - started };
      stages.push(record); log(record); return value;
    } catch (error) {
      const record = { name, passed: false, duration_ms: Date.now() - started, error: error.message };
      stages.push(record); log(record); throw error;
    }
  }
  async function raw(url, { method = 'GET', body, timeout = 30_000 } = {}) {
    loopback(url);
    try {
      const response = await fetch(url, {
        method, redirect: 'error',
        headers: body === undefined ? {} : { 'content-type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.any([fullTimeout, AbortSignal.timeout(timeout)]),
      });
      const text = await response.text();
      transport.push({ method, url, status: response.status, body: text });
      let value;
      try { value = JSON.parse(text); } catch { value = text; }
      return { status: response.status, ok: response.ok, value };
    } catch (error) {
      transport.push({ method, url, error: error.message }); throw error;
    }
  }
  async function api(url, options) {
    const response = await raw(url, options);
    assert(response.ok && response.value?.ok === true, `Request failed ${url}: ${JSON.stringify(response)}`);
    return response.value.data;
  }
  const post = (path, body) => api(`${hiveUrl}${path}`, { method: 'POST', body });
  const lookup = key => api(`${hiveUrl}/v1/entity-creations/${key}`);
  const profileUrl = entity => `${hiveUrl}/v1/entities/${entity.id}/presence`;
  async function registry() {
    const rows = await api(`${hiveUrl}/v1/entities`);
    assert(Array.isArray(rows), 'Entity listing must be an array');
    return rows;
  }
  async function familyIds() {
    return (await registry()).filter(row => !row.is_hive).map(row => row.id).sort();
  }
  async function create(name, key) {
    const entity = await post('/v1/entities', { name, creation_key: key });
    assert.match(entity.id, /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/);
    assert.equal(entity.creation_key, key);
    const child = relative(dataDir, resolve(entity.directory));
    assert(child && !child.startsWith('..') && !isAbsolute(child), 'Returned Entity directory left the isolated data root');
    return { ...entity, name, key };
  }
  async function stopOwned() {
    // Run even after Hive exits, to collect only its matching supervised children.
    log({ scoped_process_cleanup: await stopIsolatedWindowsDaemons({ binaryDir, dataDir }) });
    for (const child of ownedProcesses) {
      await poll(() => child.exitCode !== null || child.signalCode !== null,
        `Owned Hive ${child.pid} did not exit`, 10_000);
    }
    ownedProcesses.clear();
  }
  async function startHive() {
    const port = await freePort();
    hiveUrl = `http://127.0.0.1:${port}`;
    const child = spawn(join(binaryDir, 'hive-daemon.exe'), ['--port', String(port), '--data-dir', dataDir], {
      cwd: runDir, env: environment, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    });
    ownedProcesses.add(child);
    let spawnError;
    child.on('error', error => { spawnError = error; });
    const stdout = createWriteStream(join(runDir, `hive-${generation}.stdout.txt`));
    const stderr = createWriteStream(join(runDir, `hive-${generation++}.stderr.txt`));
    child.stdout.pipe(stdout); child.stderr.pipe(stderr);
    return poll(async () => {
      if (spawnError) throw spawnError;
      if (child.exitCode !== null || child.signalCode !== null) throw new Error(`Hive exited: ${child.exitCode ?? child.signalCode}`);
      const response = await raw(`${hiveUrl}/health`, { timeout: 1_500 }).catch(() => null);
      if (!response?.ok) return null;
      const status = await api(`${hiveUrl}/v1/status`, { timeout: 2_000 });
      return status.helper?.running && status.helper.local_url ? status : null;
    }, 'Hive helper did not become available', 60_000);
  }
  async function open(entity) {
    const response = await post(`/v1/entities/${entity.id}/open`, {});
    assert.equal(response.entity_id, entity.id);
    loopback(response.local_url);
    entity.url = response.local_url;
    const status = await api(`${entity.url}/v1/status`);
    assert.equal(status.entity_id, entity.id);
  }
  const history = entity => api(`${entity.url}/v1/chat/history?session_id=main&limit=200`);
  async function stream(entity, marker) {
    const url = `${entity.url}/v1/chat/stream`;
    loopback(url);
    let events = '';
    try {
      const response = await fetch(url, {
        method: 'POST', redirect: 'error', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ message: marker, session_id: 'main' }),
        signal: AbortSignal.any([fullTimeout, AbortSignal.timeout(45_000)]),
      });
      assert(response.ok, `SSE HTTP ${response.status}`);
      events = await response.text();
      const frames = events.split(/\r?\n\r?\n/).map(frame => ({
        event: frame.match(/^event:\s*(.+)$/m)?.[1]?.trim(),
        data: frame.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).trimStart()).join('\n'),
      }));
      assert(!frames.some(frame => frame.event === 'error'), `SSE error: ${events}`);
      const done = frames.find(frame => frame.event === 'done');
      assert(done, 'SSE ended without a done event');
      assert(JSON.parse(done.data).reply.includes(marker), 'SSE final reply did not come from fixture');
      assert(frames.filter(frame => frame.event === 'token').map(frame => frame.data).join('').includes(marker), 'SSE tokens missing');
    } finally { transport.push({ method: 'POST SSE', url, events }); }
  }
  try {
    fixture = await startModelFixture({ logPath: join(runDir, 'fixture-requests.jsonl') });
    await stage('start isolated Hive and helper', startHive);
    await stage('configure only the synthetic loopback model', async () => {
      const provider = await post('/v1/providers/local', { base_url: fixture.url });
      assert.equal(provider.model, fixture.model);
      assert.deepEqual(await familyIds(), []);
    });
    await stage('absent lookup, keyed creation, complete lookup and exact replay', async () => {
      const key = randomUUID();
      assert.deepEqual(await lookup(key), { creation_key: key, state: 'absent' });
      const entity = await create('First Signal Alpha', key);
      entities.push(entity);
      const found = await lookup(key);
      assert.equal(found.state, 'complete'); assert.equal(found.id, entity.id);
      assert.equal(found.name, entity.name); assert.equal(found.directory, entity.directory);
      assert.equal((await create(entity.name, key)).id, entity.id);
      assert.deepEqual(await familyIds(), [entity.id]);
    });
    await stage('four concurrent same-key requests create one Entity', async () => {
      const key = randomUUID();
      const results = await settled(Array.from({ length: 4 }, () => create('First Signal Beta', key)));
      assert.equal(new Set(results.map(entity => entity.id)).size, 1);
      entities.push(results[0]);
      assert.deepEqual(await familyIds(), entities.map(entity => entity.id).sort());
    });
    await stage('name conflict and malformed key preserve the registry', async () => {
      const expected = await familyIds();
      const conflict = await raw(`${hiveUrl}/v1/entities`, { method: 'POST', body: { name: 'Different Name', creation_key: entities[0].key } });
      assert.equal(conflict.status, 409); assert.equal(conflict.value.ok, false);
      const invalid = await raw(`${hiveUrl}/v1/entities`, { method: 'POST', body: { name: 'Invalid Key', creation_key: 'invalid-key' } });
      assert.equal(invalid.status, 400); assert.equal(invalid.value.ok, false);
      const invalidLookup = await raw(`${hiveUrl}/v1/entity-creations/invalid-key`);
      assert.equal(invalidLookup.status, 400);
      assert.deepEqual(await familyIds(), expected);
    });
    await stage('presence save/reload and rejected update preserve display metadata', async () => {
      for (const [index, entity] of entities.entries()) {
        const defaults = await api(profileUrl(entity));
        assert.deepEqual(defaults, { name: entity.name, purpose: '', temperament: 'gentle', boundaries: '', identity: 'aether' });
        entity.profile = { name: `${entity.name} display`, purpose: 'Synthetic planning brief.', temperament: 'curious', boundaries: 'Ask before acting.\nShow sources.', identity: index ? 'ember' : 'lumen' };
        assert.deepEqual(await api(profileUrl(entity), { method: 'PUT', body: entity.profile }), entity.profile);
        assert.deepEqual(await api(profileUrl(entity)), entity.profile);
        const rejected = await raw(profileUrl(entity), { method: 'PUT', body: { ...entity.profile, name: '' } });
        assert.equal(rejected.value.ok, false);
        assert.deepEqual(await api(profileUrl(entity)), entity.profile);
        const registered = await api(`${hiveUrl}/v1/entities/${entity.id}`);
        assert.equal(registered.name, entity.name, 'Presence display label changed registered identity');
        assert.equal(registered.birth_complete, false, 'Presence save unexpectedly performed birth');
      }
    });
    await stage('default quickstart birth, certificate and isolated Documents', async () => {
      for (const entity of entities) {
        // Presence purpose/boundaries are UI notes; never inject them into birth.
        const born = await post(`/v1/entities/${entity.id}/birth`, { path: 'quickstart', choices: [] });
        assert.equal(born.certificate.entity_id, entity.id);
        assert(born.certificate.signature, 'Birth certificate missing signature');
        const document = await api(`${hiveUrl}/v1/entities/${entity.id}/birth`);
        assert.equal(document.entity.birth_complete, true);
        assert.equal(document.certificate.entity_id, entity.id);
        assert(existsSync(join(documentsDir, 'Abigail', entity.name)));
      }
      assert(existsSync(join(documentsDir, 'Abigail', 'Abigail Hive')));
    });
    await stage('open both Entities and complete fixture chat and SSE', async () => {
      await settled(entities.map(open));
      assert.equal(new Set(entities.map(entity => entity.url)).size, entities.length);
      await settled(entities.map(async entity => {
        entity.marker = `FIRST_SIGNAL_${entity.id}_${runId}`;
        const chat = await api(`${entity.url}/v1/chat`, { method: 'POST', body: { message: entity.marker, session_id: 'main' }, timeout: 45_000 });
        assert(chat.reply.includes(entity.marker), 'Chat did not return the synthetic fixture text');
        await stream(entity, `${entity.marker}_STREAM`);
      }));
    });
    await stage('persist exactly two turns per Entity and isolate histories', async () => {
      for (const entity of entities) {
        entity.history = await history(entity);
        assert.equal(entity.history.session_id, 'main');
        assert.equal(entity.history.messages.length, 4);
        const contents = entity.history.messages.map(message => message.content).join('\n');
        assert(contents.includes(entity.marker));
        for (const other of entities) if (other !== entity) assert(!contents.includes(other.marker));
      }
    });
    await stage('close/reopen retains history and presence', async () => {
      for (const entity of entities) {
        await post(`/v1/entities/${entity.id}/close`, {});
        await open(entity);
        assert.deepEqual((await history(entity)).messages, entity.history.messages);
        assert.deepEqual(await api(profileUrl(entity)), entity.profile);
      }
    });
    await stage('restart same data and replay original keys without duplicates or data loss', async () => {
      await stopOwned();
      await startHive();
      for (const entity of entities) {
        const replay = await create(entity.name, entity.key);
        assert.equal(replay.id, entity.id); assert.equal(replay.directory, entity.directory);
        assert.equal((await lookup(entity.key)).id, entity.id);
        assert.deepEqual(await api(profileUrl(entity)), entity.profile);
        await open(entity);
        assert.deepEqual((await history(entity)).messages, entity.history.messages);
      }
      assert.deepEqual(await familyIds(), entities.map(entity => entity.id).sort());
      assert(existsSync(join(dataDir, 'memory.db', 'wal')), 'Isolated durable database WAL missing');
    });
    result.passed = true;
    result.entities = entities.map(({ id, name, key, directory }) => ({ id, name, creation_key: key, directory }));
  } catch (error) {
    result.error = error.stack;
    console.error(error.stack);
    process.exitCode = 1;
  } finally {
    try { if (ownedProcesses.size) await stopOwned(); } catch (error) {
      result.cleanup_error = error.message; result.passed = false; process.exitCode = 1;
    }
    try { if (fixture) await fixture.close(); } catch (error) {
      result.fixture_cleanup_error = error.message; result.passed = false; process.exitCode = 1;
    }
    // Never persist environment/raw test key. Retain synthetic evidence for review.
    delete environment.ABIGAIL_VAULT_RAW_KEY;
    writeFileSync(join(runDir, 'http-traces.json'), JSON.stringify(transport, null, 2));
    writeFileSync(join(runDir, 'result.json'), JSON.stringify(result, null, 2));
    console.log(`Synthetic acceptance ${result.passed ? 'PASS' : 'FAIL'}: ${join(runDir, 'result.json')}`);
  }
}

function loopback(value) {
  const url = new URL(value);
  assert(url.protocol === 'http:' && url.hostname === '127.0.0.1' && !url.username && !url.password,
    `Expected synthetic loopback URL: ${value}`);
}
async function poll(action, message, timeout) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = await action();
    if (value) return value;
    await delay(200);
  }
  throw new Error(message);
}
async function settled(promises) {
  const results = await Promise.allSettled(promises);
  const errors = results.filter(result => result.status === 'rejected').map(result => result.reason);
  if (errors.length) throw new AggregateError(errors, errors.map(error => error.message).join('; '));
  return results.map(result => result.value);
}
async function freePort() {
  const server = createServer();
  await new Promise((resolvePromise, reject) => {
    server.once('error', reject); server.listen(0, '127.0.0.1', resolvePromise);
  });
  const port = server.address().port;
  await new Promise((resolvePromise, reject) => server.close(error => error ? reject(error) : resolvePromise()));
  return port;
}
async function hashFile(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}
