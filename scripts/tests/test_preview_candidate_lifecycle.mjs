// Credential-free tests: no normal binaries, keyring/DPAPI, installer or signing.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  PRODUCT_SOURCE, Api, authorizeExecution, boundedText, childEnvironment,
  daemonArguments, endpoint, inside, parseArguments, parseSse, pins,
  processIdentity, profileMarker, publicReport, uuid, windowsPath,
} from './preview-candidate-contract.mjs';
import { Lifecycle, detachUncertainProcesses } from './preview-candidate-lifecycle.mjs';

const P = 'a7d1dfce-2ba2-4ec2-82af-867692509eac';
const H = '548bc060-b875-42c4-b77c-3d6c1732cc47';
const E = 'e276dce0-9fd0-443b-a83c-a8f97eeaa680';
const F = '3f0be725-7cc7-4b3d-b1ac-366ae58652dd';
const C = 'f9cc0050-5389-4fd4-976f-e32503ed6775';
const ROOT = 'C:\\runner\\temp\\abigail-preview-123-1';
const R = `${ROOT}\\profiles\\preview`;
const identity = { channel: 'beta', profile_id: P, canonical_data_root: R, instance_id: H, hive_instance_id: H };
const expected = { root: R, profileId: P, hiveInstance: H, instance: H };
const options = {
  execute: true, approvedDpapi: true, phase: 'seed', productSource: PRODUCT_SOURCE,
  fixtureRoot: ROOT, previewInstall: `${ROOT}\\install-preview`,
  stableInstall: `${ROOT}\\install-stable`, recoveryCli: `${ROOT}\\control\\abigail-profile.exe`,
  report: `${ROOT}\\control\\seed-report.json`,
};
const hosted = {
  GITHUB_ACTIONS: 'true', GITHUB_REPOSITORY: 'jbcupps/abigail', RUNNER_ENVIRONMENT: 'github-hosted',
  RUNNER_OS: 'Windows', RUNNER_TEMP: 'C:\\runner\\temp', GITHUB_RUN_ID: '123',
};
const envInput = {
  ambient: { SystemRoot: 'C:\\Windows', PATH: 'C:\\provider-cli', SSL_COM_PASSWORD: 'DO_NOT_COPY',
    OPENAI_API_KEY: 'DO_NOT_COPY', ABIGAIL_CI_MODE: '1', ABIGAIL_DAEMON_INTEGRATION: '1',
    ABIGAIL_VAULT_KEY: 'DO_NOT_COPY', ABIGAIL_HIVE_URL: 'http://127.0.0.1:43141' },
  fixtureRoot: ROOT, installRoot: options.previewInstall, dataRoot: R, channel: 'preview',
  identity: { profileId: P, hiveInstance: H },
};
function code(value) { return error => error.code === value; }
function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
}

test('import and default arguments select inert plan', () => {
  assert.deepEqual(parseArguments([]), { execute: false, approvedDpapi: false, phase: 'seed' });
  assert.equal(parseArguments(['--plan']).execute, false);
});
test('parser refuses duplicate, unknown, missing-value and contradictory options', () => {
  for (const args of [['--execute', '--execute'], ['--run'], ['--phase'], ['--phase', 'all'],
    ['--execute', '--plan'], ['--fixture-root', '--execute']]) {
    assert.throws(() => parseArguments(args));
  }
});
test('CLI camel-case mapping matches orchestrator interface', () => {
  const parsed = parseArguments(['--execute', '--approved-dpapi', '--phase', 'verify',
    '--fixture-root', ROOT, '--preview-install', options.previewInstall,
    '--stable-install', options.stableInstall, '--recovery-cli', options.recoveryCli,
    '--report', options.report, '--product-source', PRODUCT_SOURCE]);
  assert.deepEqual(parsed, { ...options, phase: 'verify' });
});
test('exact approved hosted fixture passes; helper metadata is required', () => {
  authorizeExecution(options, hosted, 'win32');
  for (const field of ['GITHUB_ACTIONS', 'GITHUB_REPOSITORY', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'GITHUB_RUN_ID']) {
    assert.throws(() => authorizeExecution(options, { ...hosted, [field]: '' }, 'win32'));
  }
});
test('guards refuse local execution, absent approval and wrong source', () => {
  for (const change of [{ execute: false }, { approvedDpapi: false }, { productSource: '0'.repeat(40) }]) {
    assert.throws(() => authorizeExecution({ ...options, ...change }, hosted, 'win32'));
  }
  assert.throws(() => authorizeExecution(options, hosted, 'linux'));
});
test('recovery CLI must be the verified in-fixture copy, not a sibling build artifact', () => {
  assert.throws(() => authorizeExecution({ ...options,
    recoveryCli: 'C:\\runner\\temp\\abigail-preview-build\\preview\\release\\abigail-profile.exe' }, hosted, 'win32'),
  code('path-outside-fixture'));
});
test('profile/install/report roots cannot escape or alias the fixture', () => {
  for (const change of [{ fixtureRoot: hosted.RUNNER_TEMP }, { previewInstall: `${ROOT}\\..\\prod` },
    { stableInstall: options.previewInstall }, { report: 'C:\\Users\\Jim\\report.json' }]) {
    assert.throws(() => authorizeExecution({ ...options, ...change }, hosted, 'win32'));
  }
  assert.throws(() => inside(ROOT, `${ROOT}-other\\profile`));
});
test('Windows extended canonical form matches exact local drive root', () => {
  assert.equal(windowsPath(`\\\\?\\${R}`), windowsPath(R.toUpperCase()));
  for (const value of ['\\\\server\\share\\profile', 'relative', '\\Device\\profile', 'C:\\bad\0']) {
    assert.throws(() => windowsPath(value));
  }
});
test('marker uses supplied dynamic nonnil UUID and implemented schema', () => {
  assert.deepEqual(profileMarker(P), { schema_version: 1, kind: 'preview', channel: 'beta', profile_id: P });
  assert.equal(uuid('00000000-0000-0000-0000-000000000000'), false);
  assert.throws(() => profileMarker(P.toUpperCase()));
});
test('complete process identity is accepted in canonical Windows form', () => {
  assert.deepEqual(processIdentity(identity, expected), identity);
  processIdentity({ ...identity, canonical_data_root: `\\\\?\\${R}` }, expected);
});
for (const [field, value, error] of [
  ['channel', 'stable', 'invalid-process-identity'],
  ['profile_id', E, 'wrong-profile-identity'],
  ['canonical_data_root', `${ROOT}\\profiles\\stable`, 'wrong-profile-root'],
  ['instance_id', E, 'stale-process-instance'],
  ['hive_instance_id', E, 'wrong-hive-instance'],
]) {
  test(`every process_identity field checked: ${field}`, () => {
    assert.throws(() => processIdentity({ ...identity, [field]: value }, expected), code(error));
    const missing = { ...identity }; delete missing[field];
    assert.throws(() => processIdentity(missing, expected));
  });
}
test('unknown process identity fields and nil UUID cannot be silently accepted', () => {
  assert.throws(() => processIdentity({ ...identity, extra: true }, expected));
  assert.throws(() => processIdentity({ ...identity, instance_id: '00000000-0000-0000-0000-000000000000' }, expected));
});
test('endpoint guard refuses remote, credentials, TLS, paths and preview reserved ports', () => {
  assert.equal(endpoint('http://127.0.0.1:49321/', true), 'http://127.0.0.1:49321');
  for (const value of ['http://localhost:49321', 'https://127.0.0.1:49321',
    'http://127.0.0.1:43141', 'http://127.0.0.1:43150', 'http://u:p@127.0.0.1:49321',
    'http://127.0.0.1:49321/v1', 'http://127.0.0.1:49321?host=external', 'http://127.0.0.1:49321#x']) {
    assert.throws(() => endpoint(value, true));
  }
});
test('preview launch arguments include explicit profile/root and dynamic nonreserved port', () => {
  assert.deepEqual(daemonArguments('preview', R, 49321),
    ['--profile', 'preview', '--data-dir', R, '--port', '49321']);
  assert.deepEqual(daemonArguments('stable', R, 43141), ['--data-dir', R, '--port', '43141']);
  assert.throws(() => daemonArguments('preview', R, 43141));
});
test('child environment is an allowlist with no credentials, account CLIs or test-mode bypass', () => {
  const env = childEnvironment(envInput);
  for (const key of ['SSL_COM_PASSWORD', 'OPENAI_API_KEY', 'ABIGAIL_CI_MODE', 'ABIGAIL_VAULT_KEY',
    'ABIGAIL_DAEMON_INTEGRATION', 'ABIGAIL_HIVE_URL']) assert.equal(Object.hasOwn(env, key), false);
  assert.equal(env.PATH, `${options.previewInstall}\\resources;C:\\Windows\\System32`);
  assert.equal(env.ABIGAIL_DOCUMENTS_DIR, `${R}\\Documents`);
  assert.equal(env.ABIGAIL_PROFILE_ID, P);
  assert.equal(env.ABIGAIL_HIVE_INSTANCE_ID, H);
  assert.equal(env.ABIGAIL_PROCESS_INSTANCE_ID, H);
  assert.equal(JSON.stringify(env).includes('DO_NOT_COPY'), false);
  for (const key of ['HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP']) inside(ROOT, env[key]);
});
test('stable environment matches owned helper allowed keys and has no preview selectors', () => {
  const env = childEnvironment({ ...envInput, channel: 'stable',
    installRoot: options.stableInstall, dataRoot: `${ROOT}\\profiles\\stable` });
  for (const key of ['ABIGAIL_PROFILE', 'ABIGAIL_PROFILE_ID', 'ABIGAIL_DATA_DIR',
    'ABIGAIL_PROCESS_INSTANCE_ID', 'ABIGAIL_HIVE_INSTANCE_ID']) assert.equal(Object.hasOwn(env, key), false);
  assert.equal(env.ABIGAIL_ENTITY_DAEMON_PATH, `${options.stableInstall}\\resources\\entity-daemon.exe`);
});
test('API sends exact two routing pins and disables redirects', async () => {
  let observed;
  const api = new Api('http://127.0.0.1:49321', identity, async (url, request) => {
    observed = { url, request }; return jsonResponse({ ok: true, data: { id: E } });
  });
  assert.deepEqual(await api.call('/v1/entities', { method: 'POST', body: { name: 'Synthetic' } }), { id: E });
  assert.equal(observed.request.redirect, 'error');
  assert.deepEqual(observed.request.headers, { 'content-type': 'application/json', ...pins(identity) });
  assert.equal(observed.url, 'http://127.0.0.1:49321/v1/entities');
});
test('API refuses redirect statuses and masks arbitrary transport secrets', async () => {
  const redirect = new Api('http://127.0.0.1:49321', identity,
    async () => new Response('', { status: 302, headers: { location: 'https://example.invalid' } }));
  await assert.rejects(redirect.call('/v1/entities'), code('redirect-refused'));
  const failing = new Api('http://127.0.0.1:49321', identity, async () => { throw Error('SECRET-BEARER'); });
  await assert.rejects(failing.call('/v1/entities'), error => error.message === 'api-transport-failed');
});
test('API distinguishes rejected envelope, rejected status and malformed body', async () => {
  for (const result of [jsonResponse({ ok: false, error: 'PRIVATE' }),
    jsonResponse({ ok: true, data: {} }, 409), jsonResponse({ ok: true })]) {
    await assert.rejects(new Api('http://127.0.0.1:49321', identity, async () => result).call('/v1/entities'),
      code('api-request-refused'));
  }
});
test('bounded HTTP reader refuses oversized body', async () => {
  await assert.rejects(boundedText(new Response('123456789'), 8), code('response-size-exceeded'));
});
test('SSE requires exact tokens and one complete done reply, refuses explicit errors', () => {
  assert.deepEqual(parseSse('event: token\ndata: abc\n\nevent: done\ndata: {"reply":"abc"}\n\n'), { reply: 'abc' });
  for (const stream of [
    'event: token\ndata: abc\n\n', 'event: error\ndata: PRIVATE\n\n',
    'event: token\ndata: abc\n\nevent: done\ndata: {"reply":"xyz"}\n\n',
    'event: token\ndata: abc\n\nevent: done\ndata: {"reply":"abc"}\n\nevent: done\ndata: {"reply":"abc"}\n\n',
  ]) assert.throws(() => parseSse(stream));
});
test('public report drops private fields, content and arbitrary error messages', () => {
  const report = publicReport({ phase: 'seed', passed: false, quiesced: false,
    assertions: [{ name: 'preview-acknowledged-shutdown', passed: true, transport: 'SECRET' }],
    failure: 'SECRET raw exception', environment: 'SECRET', state: 'SECRET' });
  assert.equal(JSON.stringify(report).includes('SECRET'), false);
  assert.equal(report.processes_quiesced, false);
  assert.equal(report.failure, 'unclassified-failure');
});
test('preview stop requires both acknowledged reap and actual successful child exit', async () => {
  const lifecycle = new Lifecycle(options);
  const record = { channel: 'preview', identityVerified: true, exited: false, exit: Promise.resolve(0),
    api: { call: async route => { assert.equal(route, '/v1/shutdown'); return { shutdown: true, owned_entities_reaped: true }; } } };
  await lifecycle.stop(record); assert.equal(record.stopped, true);
  for (const altered of [{ exit: Promise.resolve(1) },
    { api: { call: async () => ({ shutdown: true, owned_entities_reaped: false }) } }]) {
    const bad = { ...record, stopped: false, ...altered };
    await assert.rejects(lifecycle.stop(bad)); assert.equal(bad.stopped, false);
  }
});
test('preview failure before identity validation never contacts an unverified shutdown endpoint', async () => {
  const lifecycle = new Lifecycle(options); let requested = false;
  const record = { channel: 'preview', identityVerified: false, exited: false,
    api: { call: async () => { requested = true; } } };
  await assert.rejects(lifecycle.stop(record), code('unverified-preview-shutdown-refused'));
  assert.equal(requested, false);
});
test('uncertain timeout releases owned Node handles without killing or claiming quiescence', () => {
  const calls = [], record = { stopped: false, child: {
    stdin: { end: () => calls.push('control-eof') },
    stdout: { destroy: () => calls.push('close-stdout') },
    stderr: { destroy: () => calls.push('close-stderr') },
    unref: () => calls.push('unref'), kill: () => assert.fail('must not kill'),
  } };
  detachUncertainProcesses([record]);
  assert.deepEqual(calls, ['control-eof', 'close-stdout', 'close-stderr', 'unref']);
  assert.equal(record.stopped, false);
  assert.equal(publicReport({ phase: 'verify', passed: false, quiesced: false,
    assertions: [], failure: 'recovery-exit-timeout' }).processes_quiesced, false);
  calls.length = 0; record.stopped = true; detachUncertainProcesses([record]);
  assert.deepEqual(calls, []);
});
test('stable stop requests only exact wrapper stop and requires active owned tree count zero', async () => {
  const lifecycle = new Lifecycle(options);
  let command;
  const record = { channel: 'stable', child: { stdin: { end: value => { command = value; } } },
    exit: Promise.resolve(0), events: [{ event: 'stopped', activeProcesses: 0, mode: 'forced-owned-job' }] };
  await lifecycle.stop(record);
  assert.equal(command, 'stop\n'); assert.equal(record.stopped, true);
  await assert.rejects(lifecycle.stop({ ...record, stopped: false,
    events: [{ event: 'stopped', activeProcesses: 1, mode: 'forced-owned-job' }] }),
  code('stable-owned-tree-not-reaped'));
});
test('stable stop retry never writes a second command to an ended control pipe', async () => {
  const lifecycle = new Lifecycle(options); let writes = 0;
  const record = { channel: 'stable', stopRequested: true,
    child: { stdin: { end: () => { writes++; } } }, exit: Promise.resolve(0),
    events: [{ event: 'stopped', activeProcesses: 0, mode: 'forced-owned-job' }] };
  await lifecycle.stop(record);
  assert.equal(writes, 0); assert.equal(record.stopped, true);
});
test('open validates returned endpoint, all identities and exact entity id before use', async () => {
  const lifecycle = new Lifecycle(options); lifecycle.state = { profileId: P };
  const record = { channel: 'preview', root: R, hiveInstance: H, api: {
    call: async () => ({ entity_id: E, local_url: 'https://example.invalid', process_identity: {
      ...identity, instance_id: F,
    } }),
  } };
  await assert.rejects(lifecycle.open(record, E), code('non-loopback-endpoint'));
  record.api.call = async () => ({ entity_id: F, local_url: 'http://127.0.0.1:49322', process_identity: identity });
  await assert.rejects(lifecycle.open(record, E), code('wrong-opened-entity'));
});
test('routing refusal mock checks missing profile and missing/current process pins', async () => {
  const lifecycle = new Lifecycle(options), requests = [];
  await lifecycle.wrongPins({ identity, raw: async (route, request) => {
    requests.push({ route, request }); return { status: 409, json: { ok: false } };
  } }, '/v1/entities');
  assert.equal(requests.length, 3);
  assert.deepEqual(requests[0].request.headers, {});
  assert.notEqual(requests[1].request.headers['x-abigail-profile-id'], P);
  assert.notEqual(requests[2].request.headers['x-abigail-process-instance-id'], H);
});
test('cold restore cannot start while even one owned process lacks stopped proof', async () => {
  const lifecycle = new Lifecycle(options);
  lifecycle.owned = [{ stopped: true }, { exited: true, stopped: false }];
  let invoked = false; lifecycle.recovery = async () => { invoked = true; };
  await assert.rejects(lifecycle.coldRestore(), code('cold-operation-with-live-owner'));
  assert.equal(invoked, false);
});
test('cold restore mocked sequence preserves UUIDs and exact verified manifest', async () => {
  const lifecycle = new Lifecycle(options); lifecycle.state = { profileId: P,
    preview: { id: E, helperId: F, certificate: { master_public_key: '07'.repeat(32) } } };
  const manifest = { schema_version: 1, channel: 'beta', profile_id: P, creation_receipt_count: 1, entity_ids: [E, F] };
  const calls = [];
  lifecycle.recovery = async command => {
    calls.push(command);
    return command === 'restore' ? { profile_id: P, creation_receipt_count: 1,
      entity_ids: [E, F], destination: lifecycle.paths.restored,
      runtime_descriptors_restored: false, original_source_present_and_locked: true,
      credential_verification: { profile_id: P, credential_namespace: `preview.${P}`,
        master_public_key: Array(32).fill(7) } } : manifest;
  };
  lifecycle.start = async (channel, root) => { calls.push('start'); assert.equal(root, lifecycle.paths.restored); return {}; };
  lifecycle.verifyChannel = async () => { calls.push('verify-state'); };
  lifecycle.stop = async () => { calls.push('stop'); };
  await lifecycle.coldRestore();
  assert.deepEqual(calls, ['snapshot', 'verify', 'restore', 'start', 'verify-state', 'stop', 'verify']);
});
test('restore credential proof must bind copied profile and original master public key', async () => {
  for (const altered of [
    { profile_id: F }, { credential_namespace: 'stable' }, { master_public_key: Array(32).fill(9) },
  ]) {
    const lifecycle = new Lifecycle(options); lifecycle.state = { profileId: P,
      preview: { id: E, helperId: F, certificate: { master_public_key: '07'.repeat(32) } } };
    const manifest = { schema_version: 1, channel: 'beta', profile_id: P, creation_receipt_count: 1, entity_ids: [E, F] };
    lifecycle.recovery = async command => command === 'restore' ? {
      profile_id: P, creation_receipt_count: 1, entity_ids: [E, F], destination: lifecycle.paths.restored,
      runtime_descriptors_restored: false, original_source_present_and_locked: true,
      credential_verification: { profile_id: P, credential_namespace: `preview.${P}`,
        master_public_key: Array(32).fill(7), ...altered },
    } : manifest;
    let started = false; lifecycle.start = async () => { started = true; };
    await assert.rejects(lifecycle.coldRestore());
    assert.equal(started, false);
  }
});
test('cold restore refuses a changed manifest before calling restore', async () => {
  const lifecycle = new Lifecycle(options), calls = [];
  lifecycle.recovery = async command => { calls.push(command); return { command }; };
  await assert.rejects(lifecycle.coldRestore(), code('snapshot-verify-mismatch'));
  assert.deepEqual(calls, ['snapshot', 'verify']);
});
test('retained-state verifier checks exact certificate, receipt, presence, stale Hive and history', async () => {
  const lifecycle = new Lifecycle(options);
  const saved = { id: E, helperId: F, creationKey: C, name: 'Synthetic', hiveInstance: C,
    certificate: { signature: 'public' }, presence: { name: 'Synthetic' },
    history: [{ role: 'user', content: 'Synthetic' }] };
  lifecycle.state = { profileId: P, preview: saved };
  const visited = [];
  const record = { channel: 'preview', hiveInstance: H, api: {
    raw: async () => ({ status: 409, json: { ok: false } }),
    call: async route => {
      visited.push(route);
      if (route === '/v1/status') return { entities: [{ id: E }, { id: F }] };
      if (route.endsWith('/birth')) return { certificate: saved.certificate };
      if (route.endsWith('/presence')) return saved.presence;
      if (route.includes('entity-creations')) return { state: 'complete', id: E };
      if (route === '/v1/entities') return { id: E };
      throw Error('Unexpected mock route');
    },
  } };
  lifecycle.open = async () => ({ id: E });
  lifecycle.history = async () => saved.history;
  lifecycle.close = async () => { visited.push('close'); };
  await lifecycle.verifyChannel(record);
  assert.equal(visited.at(-1), 'close');
  lifecycle.history = async () => [];
  await assert.rejects(lifecycle.verifyChannel(record), code('retained-history-changed'));
});
