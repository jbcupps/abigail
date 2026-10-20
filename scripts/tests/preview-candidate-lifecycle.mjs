#!/usr/bin/env node
// Prepared acceptance tooling. Normal binaries are invoked only after all hosted
// execution guards succeed; --plan (the default) is side-effect free.
import fs from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { startModelFixture } from './mvp-model-fixture.mjs';
import {
  PRODUCT_SOURCE, STABLE_SOURCE, LIMITATIONS, Api, check, equal, uuid,
  windowsPath, inside, endpoint, processIdentity, pins, profileMarker,
  parseArguments, authorizeExecution, childEnvironment, daemonArguments, publicReport,
} from './preview-candidate-contract.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
async function deadline(promise, milliseconds, code) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => { const error = new Error(code); error.code = code; reject(error); }, milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}
async function noLinks(root, target) {
  inside(root, target);
  const relative = path.relative(root, target);
  let current = root;
  for (const part of ['', ...relative.split(path.sep)]) {
    current = part ? path.join(current, part) : current;
    try {
      const stat = await fs.lstat(current);
      check(!stat.isSymbolicLink(), 'linked-fixture-path');
      const canonical = await fs.realpath(current);
      equal(windowsPath(canonical), windowsPath(current), 'redirected-fixture-path');
    } catch (error) { if (error.code === 'ENOENT') break; throw error; }
  }
}
async function freshDirectory(target) {
  try { await fs.mkdir(target); } catch { check(false, 'fresh-directory-required'); }
}
async function readJson(target) {
  const stat = await fs.stat(target); check(stat.size < 16_000_000, 'private-state-too-large');
  try { return JSON.parse(await fs.readFile(target, 'utf8')); } catch { check(false, 'invalid-private-state'); }
}
async function writeNewJson(target, value) {
  await fs.writeFile(target, JSON.stringify(value, null, 2), { flag: 'wx', mode: 0o600 });
}
async function freePort(preview) {
  const server = net.createServer();
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return preview && port >= 43141 && port <= 43150 ? freePort(preview) : port;
}
function childRecord(child) {
  const record = { child, exited: false, code: undefined };
  record.exit = new Promise(resolve => {
    child.once('error', () => { record.exited = true; record.code = -1; resolve(-1); });
    child.once('exit', code => { record.exited = true; record.code = code; });
    child.once('close', code => { record.exited = true; record.code = code; resolve(code); });
  });
  return record;
}
// A failed fixture is never reported cold. Release only our own Node handles so
// the orchestrator can receive the failure and refuse installers/cleanup. EOF
// asks the stable wrapper to close its private job; no PID is discovered/killed.
export function detachUncertainProcesses(records) {
  for (const record of records) {
    if (record.stopped) continue;
    try { if (!record.child.stdin?.writableEnded) record.child.stdin?.end(); } catch { /* retain uncertain report */ }
    for (const stream of [record.child.stdout, record.child.stderr]) {
      try { stream?.destroy(); } catch { /* retain uncertain report */ }
    }
    try { record.child.unref(); } catch { /* retain uncertain report */ }
  }
}
async function poll(action, milliseconds = 90_000) {
  const until = Date.now() + milliseconds;
  while (true) {
    try { const result = await action(); if (result) return result; }
    catch (error) { if (error.code !== 'api-transport-failed') throw error; }
    check(Date.now() < until, 'readiness-timeout');
    await delay(250);
  }
}

export class Lifecycle {
  constructor(options) {
    this.options = options; this.owned = []; this.assertions = []; this.stageName = 'initialization';
    this.root = options.fixtureRoot;
    this.paths = {
      stable: path.join(this.root, 'profiles', 'stable'),
      preview: path.join(this.root, 'profiles', 'preview'),
      restored: path.join(this.root, 'profiles', 'restored-preview'),
      snapshot: path.join(this.root, 'snapshots', 'preview'),
      state: path.join(this.root, 'control', 'lifecycle-state.json'),
    };
  }
  async stage(name, action) {
    this.stageName = name;
    try { const result = await action(); this.assertions.push({ name, passed: true }); return result; }
    catch (error) { this.assertions.push({ name, passed: false }); throw error; }
  }
  async environment(channel, root, hiveInstance) {
    const env = childEnvironment({
      ambient: process.env, fixtureRoot: this.root,
      installRoot: this.options[`${channel}Install`], dataRoot: root, channel,
      identity: { profileId: this.state.profileId, hiveInstance },
    });
    for (const key of ['USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'ABIGAIL_DOCUMENTS_DIR']) {
      await noLinks(this.root, env[key]);
      await fs.mkdir(env[key], { recursive: true });
    }
    return env;
  }
  async start(channel, root = this.paths[channel]) {
    const preview = channel === 'preview', hiveInstance = randomUUID(), port = await freePort(preview);
    const environment = await this.environment(channel, root, hiveInstance);
    const installRoot = this.options[`${channel}Install`];
    const executable = path.join(installRoot, 'resources', 'hive-daemon.exe');
    await noLinks(this.root, executable); await noLinks(this.root, root);
    let record;
    if (preview) {
      record = childRecord(spawn(executable, daemonArguments(channel, root, port), {
        env: environment, cwd: installRoot, windowsHide: true, stdio: 'ignore',
      }));
    } else {
      const configPath = path.join(this.root, 'control', `stable-job-${randomUUID()}.json`);
      await writeNewJson(configPath, {
        schemaVersion: 1, fixtureRoot: this.root, installRoot, dataRoot: root,
        workingDirectory: installRoot, port, environment,
      });
      // Wrapper receives only OS essentials + hosted guard metadata, never the
      // workflow's signing secrets/provider/account environment.
      const wrapperEnvironment = { ...environment };
      for (const key of ['GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_TEMP',
        'GITHUB_RUN_ID', 'GITHUB_REPOSITORY']) {
        wrapperEnvironment[key] = process.env[key];
      }
      const pwsh = path.join(process.env.ProgramFiles, 'PowerShell', '7', 'pwsh.exe');
      record = childRecord(spawn(pwsh, ['-NoProfile', '-NonInteractive', '-File',
        path.join(here, 'preview-owned-job.ps1'), '-Execute', '-ApproveDpapiFixtures',
        '-ConfigPath', configPath], {
        env: wrapperEnvironment, cwd: installRoot, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'],
      }));
      record.child.stdin.on('error', () => { record.controlFailed = true; });
      record.events = [];
      let pending = '';
      record.child.stdout.setEncoding('utf8');
      record.child.stdout.on('data', chunk => {
        pending += chunk;
        if (pending.length > 16_384) { record.protocolInvalid = true; pending = ''; return; }
        while (pending.includes('\n')) {
          const position = pending.indexOf('\n'), line = pending.slice(0, position).trim();
          pending = pending.slice(position + 1);
          if (!line.startsWith('ABIGAIL_JOB:')) { if (line) record.protocolInvalid = true; continue; }
          try { record.events.push(JSON.parse(line.slice('ABIGAIL_JOB:'.length))); }
          catch { record.protocolInvalid = true; }
        }
      });
    }
    record.channel = channel; record.root = root; record.port = port; record.hiveInstance = hiveInstance;
    this.owned.push(record);
    if (!preview) {
      await poll(async () => {
        check(!record.exited && !record.protocolInvalid, 'stable-owned-job-failed');
        const event = record.events.find(item => item.event === 'owned');
        if (!event) return false;
        check(Number.isInteger(event.pid) && event.pid > 0 && event.port === port, 'stable-port-owner-unverified');
        return true;
      }, 120_000);
    }
    const identity = preview ? {
      channel: 'beta', profile_id: this.state.profileId, canonical_data_root: root,
      instance_id: hiveInstance, hive_instance_id: hiveInstance,
    } : undefined;
    record.api = new Api(`http://127.0.0.1:${port}`, identity);
    await poll(async () => {
      check(!record.exited, 'daemon-exited-before-ready');
      const status = await record.api.call('/v1/status');
      if (preview) processIdentity(status.process_identity, {
        root, profileId: this.state.profileId, hiveInstance, instance: hiveInstance,
      });
      check(status.master_key_loaded === true, 'fixture-master-key-unavailable');
      record.status = status;
      record.identityVerified = true;
      return true;
    });
    await this.helper(record);
    return record;
  }
  async helper(record) {
    const status = await poll(async () => {
      const status = await record.api.call('/v1/status');
      if (record.channel === 'preview') processIdentity(status.process_identity, {
        root: record.root, profileId: this.state.profileId,
        hiveInstance: record.hiveInstance, instance: record.hiveInstance,
      });
      return status.helper?.running && status.helper.local_url ? status : false;
    });
    const helper = status.entities.find(entity => entity.is_hive);
    check(helper && uuid(helper.id), 'missing-hive-helper');
    if (this.state[record.channel]?.helperId) equal(helper.id, this.state[record.channel].helperId, 'helper-id-changed');
    let identity;
    if (record.channel === 'preview') {
      identity = processIdentity(status.helper.process_identity, {
        root: record.root, profileId: this.state.profileId, hiveInstance: record.hiveInstance,
      });
      check(identity.instance_id !== record.hiveInstance, 'helper-instance-reused-hive');
    }
    const api = new Api(endpoint(status.helper.local_url, Boolean(identity)), identity);
    const helperStatus = await api.call('/v1/status');
    equal(helperStatus.entity_id, helper.id, 'wrong-helper-entity');
    if (identity) processIdentity(helperStatus.process_identity, {
      root: record.root, profileId: this.state.profileId, hiveInstance: record.hiveInstance,
      instance: identity.instance_id,
    });
    record.helperId = helper.id;
    return helper.id;
  }
  async stop(record) {
    if (record.stopped) return;
    check(!record.exited, 'daemon-exited-without-shutdown');
    if (record.channel === 'preview') {
      check(record.identityVerified === true, 'unverified-preview-shutdown-refused');
      const ack = await record.api.call('/v1/shutdown', { method: 'POST', body: {} });
      check(ack.shutdown === true && ack.owned_entities_reaped === true, 'shutdown-not-acknowledged');
      equal(await deadline(record.exit, 45_000, 'preview-exit-timeout'), 0, 'preview-exit-failed');
    } else {
      if (!record.stopRequested) {
        record.stopRequested = true;
        record.child.stdin.end('stop\n');
      }
      equal(await deadline(record.exit, 45_000, 'stable-job-exit-timeout'), 0, 'stable-job-wrapper-failed');
      check(!record.protocolInvalid && record.events.some(event => event.event === 'stopped'
        && event.activeProcesses === 0 && event.mode === 'forced-owned-job'), 'stable-owned-tree-not-reaped');
    }
    record.stopped = true;
  }
  async open(record, id) {
    const result = await record.api.call(`/v1/entities/${id}/open`, { method: 'POST', body: {} });
    equal(result.entity_id, id, 'wrong-opened-entity');
    let identity;
    if (record.channel === 'preview') {
      identity = processIdentity(result.process_identity, {
        root: record.root, profileId: this.state.profileId, hiveInstance: record.hiveInstance,
      });
      check(identity.instance_id !== record.hiveInstance, 'entity-instance-reused-hive');
    }
    const api = new Api(endpoint(result.local_url, Boolean(identity)), identity);
    const status = await api.call('/v1/status');
    equal(status.entity_id, id, 'wrong-entity-status');
    check(status.birth_complete === true, 'entity-birth-incomplete');
    if (identity) processIdentity(status.process_identity, {
      root: record.root, profileId: this.state.profileId, hiveInstance: record.hiveInstance,
      instance: identity.instance_id,
    });
    return { id, api, identity };
  }
  async history(entity) {
    const history = await entity.api.call('/v1/chat/history?session_id=main&limit=200');
    check(history.session_id === 'main' && Array.isArray(history.messages), 'invalid-history');
    return history.messages;
  }
  async close(record, entity) {
    await record.api.call(`/v1/entities/${entity.id}/close`, { method: 'POST',
      body: entity.identity ? { expected_entity_instance_id: entity.identity.instance_id } : {} });
  }
  async wrongPins(api, protectedRoute) {
    for (const headers of [{}, { ...pins(api.identity), 'x-abigail-profile-id': randomUUID() },
      { ...pins(api.identity), 'x-abigail-process-instance-id': randomUUID() }]) {
      const result = await api.raw(protectedRoute, { headers });
      check(result.status === 409 && result.json.ok === false, 'routing-pin-not-refused');
    }
  }
  async seedChannel(record) {
    const channel = record.channel, creationKey = randomUUID();
    await record.api.call('/v1/providers/local', { method: 'POST', body: { base_url: this.fixture.url } });
    const body = { name: `Disposable ${channel} family`, ...(channel === 'preview' ? { creation_key: creationKey } : {}) };
    if (channel === 'preview') {
      const absent = await record.api.call(`/v1/entity-creations/${creationKey}`);
      check(absent.state === 'absent' && absent.creation_key === creationKey, 'fresh-creation-key-not-absent');
    }
    const created = await record.api.call('/v1/entities', { method: 'POST', body });
    check(uuid(created.id), 'invalid-created-entity');
    if (channel === 'preview') {
      equal((await record.api.call('/v1/entities', { method: 'POST', body })).id, created.id, 'creation-replay-changed-id');
      const receipt = await record.api.call(`/v1/entity-creations/${creationKey}`);
      check(receipt.state === 'complete' && receipt.id === created.id
        && receipt.creation_key === creationKey, 'creation-receipt-mismatch');
    }
    const birth = await record.api.call(`/v1/entities/${created.id}/birth`, {
      method: 'POST', body: { path: 'quickstart', choices: [], purpose: 'Disposable synthetic hosted acceptance only.' },
    });
    check(birth.certificate?.entity_id === created.id && /^[0-9a-f]{128}$/.test(birth.certificate.signature)
      && /^[0-9a-f]{64}$/.test(birth.certificate.master_public_key), 'birth-certificate-invalid');
    const saved = {
      id: created.id, helperId: record.helperId, name: body.name, creationKey,
      certificate: birth.certificate, marker: `SYNTHETIC_${channel}_${randomUUID()}`,
      hiveInstance: record.hiveInstance,
    };
    if (channel === 'preview') {
      saved.presence = { name: 'Disposable presence', purpose: 'Synthetic presentation only',
        temperament: 'curious', boundaries: 'No real accounts', identity: 'lumen' };
      await record.api.call(`/v1/entities/${created.id}/presence`, { method: 'PUT', body: saved.presence });
      equal(await record.api.call(`/v1/entities/${created.id}/presence`), saved.presence, 'presence-not-saved');
      const rejected = await record.api.raw(`/v1/entities/${created.id}/presence`, {
        method: 'PUT', body: { ...saved.presence, permissions: 'all' },
      });
      check(rejected.status >= 400 || rejected.json?.ok === false, 'invalid-presence-accepted');
      equal(await record.api.call(`/v1/entities/${created.id}/presence`), saved.presence, 'invalid-presence-replaced-saved');
      await this.wrongPins(record.api, '/v1/entities');
    }
    let entity = await this.open(record, created.id);
    const reply = await entity.api.call('/v1/chat', { method: 'POST', body: { message: saved.marker, session_id: 'main' } });
    equal(reply.reply, `Contract fixture reply: ${saved.marker}`, 'synthetic-reply-mismatch');
    const streamed = await entity.api.raw('/v1/chat/stream', {
      method: 'POST', body: { message: `${saved.marker}_STREAM`, session_id: 'main' }, stream: true,
    });
    equal(streamed.data.reply, `Contract fixture reply: ${saved.marker}_STREAM`, 'synthetic-stream-mismatch');
    saved.history = await this.history(entity);
    equal(saved.history, [
      { role: 'user', content: saved.marker }, { role: 'assistant', content: reply.reply },
      { role: 'user', content: `${saved.marker}_STREAM` }, { role: 'assistant', content: streamed.data.reply },
    ], 'synthetic-history-mismatch');
    if (channel === 'preview') {
      await this.wrongPins(entity.api, '/v1/chat/history?session_id=main&limit=200');
      const stale = await record.api.raw(`/v1/entities/${created.id}/close`, {
        method: 'POST', body: { expected_entity_instance_id: randomUUID() },
      });
      check(stale.json?.ok === false, 'stale-close-accepted');
      const missing = await record.api.raw(`/v1/entities/${created.id}/close`, { method: 'POST', body: {} });
      check(missing.json?.ok === false, 'missing-close-identity-accepted');
      equal(await this.history(entity), saved.history, 'refused-close-affected-live-entity');
      const prior = entity;
      await this.close(record, entity);
      entity = await this.open(record, created.id);
      check(entity.identity.instance_id !== prior.identity.instance_id, 'entity-instance-reused-after-reopen');
      const stalePins = await entity.api.raw('/v1/chat/history?session_id=main&limit=200', { headers: pins(prior.identity) });
      check(stalePins.status === 409 && stalePins.json.ok === false, 'old-instance-pins-accepted');
      equal(await this.history(entity), saved.history, 'reopen-history-changed');
    }
    await this.close(record, entity);
    this.state[channel] = saved;
  }
  async verifyChannel(record) {
    const saved = this.state[record.channel];
    const status = await record.api.call('/v1/status');
    const expected = [saved.id, saved.helperId].sort();
    equal(status.entities.map(item => item.id).sort(), expected, 'entity-registry-changed');
    const document = await record.api.call(`/v1/entities/${saved.id}/birth`);
    equal(document.certificate, saved.certificate, 'birth-certificate-changed');
    if (record.channel === 'preview') {
      check(record.hiveInstance !== saved.hiveInstance, 'hive-instance-reused-after-restart');
      const stale = await record.api.raw('/v1/entities', { headers: {
        'x-abigail-profile-id': this.state.profileId,
        'x-abigail-process-instance-id': saved.hiveInstance,
      } });
      check(stale.status === 409 && stale.json?.ok === false, 'old-hive-instance-pins-accepted');
      equal(await record.api.call(`/v1/entities/${saved.id}/presence`), saved.presence, 'presence-changed');
      const receipt = await record.api.call(`/v1/entity-creations/${saved.creationKey}`);
      check(receipt.state === 'complete' && receipt.id === saved.id, 'retained-receipt-mismatch');
      const replay = await record.api.call('/v1/entities', {
        method: 'POST', body: { name: saved.name, creation_key: saved.creationKey },
      });
      equal(replay.id, saved.id, 'retained-creation-replay-changed-id');
    }
    const entity = await this.open(record, saved.id);
    equal(await this.history(entity), saved.history, 'retained-history-changed');
    await this.close(record, entity);
  }
  async recovery(command, args) {
    // CLI stdout contains public manifest/receipt but remains private; no raw
    // stderr/env/manifest is added to uploaded reports.
    const env = await this.environment('preview', this.paths.preview, randomUUID());
    const record = childRecord(spawn(this.options.recoveryCli, [command, ...args], {
      env, cwd: path.dirname(this.options.recoveryCli), windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'],
    }));
    record.channel = 'recovery'; this.owned.push(record);
    let output = '', oversized = false;
    record.child.stdout.setEncoding('utf8');
    record.child.stdout.on('data', value => {
      if (output.length + value.length > 16_000_000) oversized = true;
      else output += value;
    });
    equal(await deadline(record.exit, 120_000, 'recovery-exit-timeout'), 0, 'recovery-command-failed');
    record.stopped = true; check(!oversized, 'recovery-output-too-large');
    try { return JSON.parse(output); } catch { check(false, 'invalid-recovery-output'); }
  }
  async coldRestore() {
    check(this.owned.every(item => item.stopped), 'cold-operation-with-live-owner');
    const snapshot = await this.recovery('snapshot', ['--source', this.paths.preview, '--output', this.paths.snapshot]);
    const verified = await this.recovery('verify', ['--snapshot', this.paths.snapshot]);
    equal(verified, snapshot, 'snapshot-verify-mismatch');
    check(snapshot.schema_version === 1 && snapshot.channel === 'beta'
      && snapshot.profile_id === this.state.profileId && snapshot.creation_receipt_count === 1,
    'snapshot-identity-mismatch');
    equal(snapshot.entity_ids.slice().sort(), [this.state.preview.id, this.state.preview.helperId].sort(),
      'snapshot-entities-mismatch');
    const receipt = await this.recovery('restore', ['--snapshot', this.paths.snapshot, '--destination', this.paths.restored]);
    check(receipt.profile_id === this.state.profileId && receipt.creation_receipt_count === 1
      && receipt.runtime_descriptors_restored === false && receipt.original_source_present_and_locked === true,
    'restore-receipt-mismatch');
    equal(receipt.entity_ids.slice().sort(), snapshot.entity_ids.slice().sort(), 'restore-entities-mismatch');
    equal(windowsPath(receipt.destination), windowsPath(this.paths.restored), 'restore-destination-mismatch');
    const proof = receipt.credential_verification;
    check(proof?.profile_id === this.state.profileId
      && proof.credential_namespace === `preview.${this.state.profileId}`
      && Array.isArray(proof.master_public_key) && proof.master_public_key.length === 32
      && proof.master_public_key.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255),
    'restore-credential-scope-mismatch');
    equal(Buffer.from(proof.master_public_key).toString('hex'),
      this.state.preview.certificate.master_public_key, 'restore-master-public-key-changed');
    const restored = await this.start('preview', this.paths.restored);
    await this.verifyChannel(restored);
    await this.stop(restored);
    equal(await this.recovery('verify', ['--snapshot', this.paths.snapshot]), snapshot, 'restore-mutated-snapshot');
  }
  async run() {
    for (const value of Object.values(this.paths)) await noLinks(this.root, value);
    for (const directory of ['control', 'profiles', 'snapshots']) {
      await fs.mkdir(path.join(this.root, directory), { recursive: true });
    }
    if (this.options.phase === 'seed') {
      await freshDirectory(this.paths.preview); await freshDirectory(this.paths.stable);
      this.state = { schemaVersion: 1, productSource: PRODUCT_SOURCE, stableSource: STABLE_SOURCE,
        root: this.root, runId: process.env.GITHUB_RUN_ID, profileId: randomUUID() };
      await writeNewJson(path.join(this.paths.preview, 'profile.json'), profileMarker(this.state.profileId));
    } else {
      this.state = await readJson(this.paths.state);
      check(this.state.schemaVersion === 1 && this.state.productSource === PRODUCT_SOURCE
        && this.state.stableSource === STABLE_SOURCE && this.state.runId === process.env.GITHUB_RUN_ID
        && uuid(this.state.profileId), 'wrong-private-state');
      equal(windowsPath(this.state.root), windowsPath(this.root), 'private-state-root-mismatch');
      for (const channel of ['stable', 'preview']) {
        check(uuid(this.state[channel]?.id) && uuid(this.state[channel]?.helperId)
          && uuid(this.state[channel]?.creationKey) && uuid(this.state[channel]?.hiveInstance)
          && Array.isArray(this.state[channel]?.history),
        'invalid-private-entity-state');
      }
    }
    this.fixture = await startModelFixture();
    if (this.options.phase === 'seed') {
      const stable = await this.stage('stable-first-start', () => this.start('stable'));
      const preview = await this.stage('preview-second-start', () => this.start('preview'));
      await this.stage('stable-synthetic-family', () => this.seedChannel(stable));
      await this.stage('preview-synthetic-family-and-routing-guards', () => this.seedChannel(preview));
      check(this.state.stable.id !== this.state.preview.id
        && this.state.stable.helperId !== this.state.preview.helperId, 'cross-profile-entity-id');
      await this.stage('preview-acknowledged-shutdown', () => this.stop(preview));
      await this.stage('stable-owned-job-stop', () => this.stop(stable));
      await writeNewJson(this.paths.state, this.state);
    } else if (this.options.phase === 'verify') {
      const preview = await this.stage('preview-first-start', () => this.start('preview'));
      const stable = await this.stage('stable-second-start', () => this.start('stable'));
      await this.stage('preview-upgrade-retained-state', () => this.verifyChannel(preview));
      await this.stage('stable-coexistence-retained-state', () => this.verifyChannel(stable));
      await this.stage('preview-acknowledged-shutdown', () => this.stop(preview));
      await this.stage('stable-owned-job-stop', () => this.stop(stable));
      await this.stage('cold-snapshot-verify-restore-and-reopen', () => this.coldRestore());
    } else {
      const stable = await this.stage('stable-return-start', () => this.start('stable'));
      await this.stage('stable-after-preview-uninstall', () => this.verifyChannel(stable));
      await this.stage('stable-owned-job-stop', () => this.stop(stable));
    }
  }
}

export async function main(args = process.argv.slice(2)) {
  const options = parseArguments(args);
  if (!options.execute) {
    process.stdout.write(JSON.stringify({
      schema_version: 1, executed: false, mode: 'plan', product_source: PRODUCT_SOURCE,
      stable_source: STABLE_SOURCE, phases: ['seed', 'verify', 'stable-return'],
      requires: ['explicit fixture DPAPI/test-identity approval', 'ephemeral GitHub-hosted Windows runner',
        'verified exact installed payloads', 'pinned recovery CLI', 'owned stable job supervisor'],
      limitations: LIMITATIONS,
    }, null, 2) + '\n');
    return 0;
  }
  authorizeExecution(options, process.env, process.platform);
  await noLinks(process.env.RUNNER_TEMP, options.fixtureRoot);
  for (const target of [options.previewInstall, options.stableInstall, options.recoveryCli, options.report]) {
    await noLinks(options.fixtureRoot, target);
  }
  const lifecycle = new Lifecycle(options);
  let passed = false, failure;
  try { await lifecycle.run(); passed = true; }
  catch (error) { failure = error.code || 'unclassified-failure'; }
  finally {
    // Attempt only an already-bound owned channel's documented stop. No PID
    // discovery, taskkill, CIM, signals or fallback process killing.
    for (const record of lifecycle.owned.slice().reverse()) {
      if (!record.stopped && record.channel !== 'recovery') {
        try { await lifecycle.stop(record); } catch { /* uncertain => fail closed */ }
      }
    }
    if (lifecycle.fixture) await lifecycle.fixture.close();
  }
  const quiesced = lifecycle.owned.every(item => item.stopped);
  const report = publicReport({
    phase: options.phase, passed: passed && quiesced, quiesced,
    assertions: lifecycle.assertions, failure,
  });
  try {
    await writeNewJson(options.report, report);
    process.stdout.write(JSON.stringify(report) + '\n');
  } finally { detachUncertainProcesses(lifecycle.owned); }
  return report.passed ? 0 : 1;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().then(code => { process.exitCode = code; }).catch(error => {
    // Never emit arbitrary child/API error text or a raw stack containing state.
    process.stderr.write(JSON.stringify({ passed: false, execution_state: 'failed-or-uncertain',
      failure: /^[a-z0-9-]+$/.test(error.code || '') ? error.code : 'lifecycle-precondition-failed' }) + '\n');
    process.exitCode = 1;
  });
}
