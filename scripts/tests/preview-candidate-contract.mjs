// Test harness contract only. Importing this module never starts an application.
import path from 'node:path';
import { isDeepStrictEqual } from 'node:util';

export const PRODUCT_SOURCE = 'f7c7848d25726480b253ec1ca637c3ffa5ea3c77';
export const STABLE_SOURCE = 'e3ad51181da1ea11bded24ca0c39e2dcfe5fa944';
export const LIMITATIONS = Object.freeze([
  'Daemon acceptance only: no Hive/Runtime GUI, WebView, default KnownFolder bootstrap or real provider acceptance.',
  'The harness creates a fresh preview marker; it does not test GUI marker creation.',
  'Stable termination is forced within the exact owned Windows Job Object, not graceful application shutdown.',
  'Install/upgrade/uninstall are separate orchestrator assertions; all ten exact-installer stages are not claimed.',
  'No cross-user/device recovery or real household data is exercised.',
]);

export function check(condition, code) {
  if (!condition) {
    const error = new Error(code);
    error.code = code;
    throw error;
  }
}
export function equal(actual, expected, code) { check(isDeepStrictEqual(actual, expected), code); }
export function uuid(value) {
  return typeof value === 'string'
    && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value)
    && value !== '00000000-0000-0000-0000-000000000000';
}
export function windowsPath(value) {
  check(typeof value === 'string' && value.length > 0 && !value.includes('\0'), 'invalid-path');
  const plain = value.startsWith('\\\\?\\') ? value.slice(4) : value;
  check(/^[a-z]:\\/i.test(plain), 'absolute-local-path-required');
  return path.win32.normalize(plain).replace(/\\+$/, '').toLowerCase();
}
export function inside(root, target) {
  const a = windowsPath(root), b = windowsPath(target);
  check(b.startsWith(`${a}\\`), 'path-outside-fixture');
  return target;
}
export function endpoint(value, preview) {
  check(typeof value === 'string', 'invalid-endpoint');
  let url;
  try { url = new URL(value); } catch { check(false, 'invalid-endpoint'); }
  check(url.protocol === 'http:' && url.hostname === '127.0.0.1'
    && !url.username && !url.password && !url.search && !url.hash
    && url.pathname === '/' && Number(url.port) >= 1 && Number(url.port) <= 65535,
  'non-loopback-endpoint');
  check(!preview || Number(url.port) < 43141 || Number(url.port) > 43150,
    'preview-reserved-port');
  return url.origin;
}
export function processIdentity(value, expected) {
  check(value && typeof value === 'object' && !Array.isArray(value), 'missing-process-identity');
  equal(Object.keys(value).sort(), [
    'canonical_data_root', 'channel', 'hive_instance_id', 'instance_id', 'profile_id',
  ], 'unexpected-process-identity-fields');
  check(value.channel === 'beta' && uuid(value.profile_id) && uuid(value.instance_id)
    && uuid(value.hive_instance_id), 'invalid-process-identity');
  equal(value.profile_id, expected.profileId, 'wrong-profile-identity');
  equal(windowsPath(value.canonical_data_root), windowsPath(expected.root), 'wrong-profile-root');
  equal(value.hive_instance_id, expected.hiveInstance, 'wrong-hive-instance');
  if (expected.instance) equal(value.instance_id, expected.instance, 'stale-process-instance');
  return { ...value };
}
export function pins(identity) {
  return identity ? {
    'x-abigail-profile-id': identity.profile_id,
    'x-abigail-process-instance-id': identity.instance_id,
  } : {};
}
export function profileMarker(profileId) {
  check(uuid(profileId), 'invalid-profile-id');
  return { schema_version: 1, kind: 'preview', channel: 'beta', profile_id: profileId };
}

const valueOptions = new Set([
  '--phase', '--preview-install', '--stable-install', '--fixture-root',
  '--recovery-cli', '--report', '--product-source',
]);
export function parseArguments(args) {
  const options = { execute: false, approvedDpapi: false, phase: 'seed' };
  const seen = new Set();
  for (let index = 0; index < args.length; index++) {
    const key = args[index];
    check(!seen.has(key), 'duplicate-option'); seen.add(key);
    if (key === '--execute') options.execute = true;
    else if (key === '--approved-dpapi') options.approvedDpapi = true;
    else if (key === '--plan') options.plan = true;
    else {
      check(valueOptions.has(key) && typeof args[index + 1] === 'string'
        && !args[index + 1].startsWith('--'), 'invalid-option');
      options[key.slice(2).replace(/-([a-z])/g, (_, char) => char.toUpperCase())] = args[++index];
    }
  }
  check(['seed', 'verify', 'stable-return'].includes(options.phase), 'invalid-phase');
  check(!(options.plan && options.execute), 'conflicting-execution-options');
  return options;
}
export function authorizeExecution(options, env, platform) {
  check(options.execute && options.approvedDpapi, 'explicit-fixture-approval-required');
  check(platform === 'win32' && env.GITHUB_ACTIONS === 'true'
    && env.RUNNER_ENVIRONMENT === 'github-hosted' && env.RUNNER_OS === 'Windows',
  'ephemeral-hosted-windows-required');
  equal(env.GITHUB_REPOSITORY, 'jbcupps/abigail', 'wrong-hosted-repository');
  equal(options.productSource, PRODUCT_SOURCE, 'wrong-product-source');
  check(typeof env.GITHUB_RUN_ID === 'string' && /^\d+$/.test(env.GITHUB_RUN_ID),
    'hosted-run-id-required');
  inside(env.RUNNER_TEMP, options.fixtureRoot);
  equal(windowsPath(path.win32.dirname(options.fixtureRoot)), windowsPath(env.RUNNER_TEMP),
    'fixture-must-be-direct-runner-child');
  check(/^abigail-preview-[A-Za-z0-9-]+$/.test(path.win32.basename(options.fixtureRoot)),
    'invalid-fixture-directory-name');
  for (const name of ['previewInstall', 'stableInstall', 'recoveryCli', 'report']) {
    inside(options.fixtureRoot, options[name]);
  }
  equal(windowsPath(options.previewInstall),
    windowsPath(path.win32.join(options.fixtureRoot, 'install-preview')), 'wrong-preview-install');
  equal(windowsPath(options.stableInstall),
    windowsPath(path.win32.join(options.fixtureRoot, 'install-stable')), 'wrong-stable-install');
}
export function childEnvironment({ ambient, fixtureRoot, installRoot, dataRoot, channel, identity }) {
  check(['preview', 'stable'].includes(channel), 'invalid-channel');
  inside(fixtureRoot, installRoot); inside(fixtureRoot, dataRoot);
  const user = path.win32.join(fixtureRoot, 'user', channel);
  const resources = path.win32.join(installRoot, 'resources');
  const result = {};
  for (const key of ['SystemRoot', 'WINDIR', 'SystemDrive', 'COMSPEC', 'PATHEXT',
    'NUMBER_OF_PROCESSORS', 'PROCESSOR_ARCHITECTURE']) {
    const actual = Object.keys(ambient).find(name => name.toLowerCase() === key.toLowerCase());
    if (actual && ambient[actual]) result[key] = ambient[actual];
  }
  check(result.SystemRoot, 'system-root-required');
  result.PATH = `${resources};${path.win32.join(result.SystemRoot, 'System32')}`;
  result.USERPROFILE = user;
  result.HOME = user;
  result.APPDATA = path.win32.join(user, 'AppData', 'Roaming');
  result.LOCALAPPDATA = path.win32.join(user, 'AppData', 'Local');
  result.TEMP = path.win32.join(user, 'Temp');
  result.TMP = result.TEMP;
  result.RUST_LOG = 'error';
  result.ABIGAIL_DOCUMENTS_DIR = path.win32.join(dataRoot, 'Documents');
  result.ABIGAIL_ENTITY_DAEMON_PATH = path.win32.join(resources, 'entity-daemon.exe');
  result.ABIGAIL_INTERNAL_BIN_DIR = resources;
  if (channel === 'preview') {
    check(identity && uuid(identity.profileId) && uuid(identity.hiveInstance), 'preview-identity-required');
    result.ABIGAIL_PROFILE = 'preview';
    result.ABIGAIL_DATA_DIR = dataRoot;
    result.ABIGAIL_PROFILE_ID = identity.profileId;
    result.ABIGAIL_HIVE_INSTANCE_ID = identity.hiveInstance;
    result.ABIGAIL_PROCESS_INSTANCE_ID = identity.hiveInstance;
  }
  return result;
}
export function daemonArguments(channel, root, port) {
  check(Number.isInteger(port) && port > 0 && port < 65536, 'invalid-port');
  endpoint(`http://127.0.0.1:${port}`, channel === 'preview');
  return channel === 'preview'
    ? ['--profile', 'preview', '--data-dir', root, '--port', String(port)]
    : ['--data-dir', root, '--port', String(port)];
}
export function parseSse(text) {
  let tokens = '', done;
  for (const block of text.replace(/\r\n/g, '\n').split('\n\n')) {
    const lines = block.split('\n');
    const event = lines.find(line => line.startsWith('event:'))?.slice(6).trim();
    const data = lines.filter(line => line.startsWith('data:')).map(line => line.slice(5).replace(/^ /, '')).join('\n');
    check(event !== 'error', 'stream-error');
    if (event === 'token') tokens += data;
    if (event === 'done') {
      check(done === undefined, 'duplicate-stream-done');
      try { done = JSON.parse(data); } catch { check(false, 'invalid-stream-done'); }
    }
  }
  check(typeof done?.reply === 'string' && tokens.length > 0 && tokens === done.reply,
    'stream-reply-mismatch');
  return done;
}
export async function boundedText(response, limit = 2_000_000) {
  check(response.body?.getReader, 'missing-response-body');
  const reader = response.body.getReader(), decoder = new TextDecoder();
  let result = '', bytes = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      check(bytes <= limit, 'response-size-exceeded');
      result += decoder.decode(value, { stream: true });
    }
    return result + decoder.decode();
  } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
}
export class Api {
  constructor(base, identity, fetchImpl = globalThis.fetch) {
    this.base = endpoint(base, Boolean(identity));
    this.identity = identity;
    this.fetchImpl = fetchImpl;
  }
  async raw(route, { method = 'GET', body, headers = pins(this.identity), stream = false } = {}) {
    check(typeof route === 'string' && route.startsWith('/v1/') && !route.includes('\\')
      && !route.includes('://'), 'invalid-api-route');
    let response;
    try {
      response = await this.fetchImpl(`${this.base}${route}`, {
        method, headers: { 'content-type': 'application/json', ...headers },
        body: body === undefined ? undefined : JSON.stringify(body),
        redirect: 'error', signal: AbortSignal.timeout(45_000),
      });
    } catch { check(false, 'api-transport-failed'); }
    check(response.status < 300 || response.status >= 400, 'redirect-refused');
    let text;
    try { text = await boundedText(response); } catch (error) {
      if (error.code) throw error; check(false, 'api-body-failed');
    }
    if (stream) {
      check(response.ok && response.headers.get('content-type')?.includes('text/event-stream'),
        'invalid-stream-response');
      return { status: response.status, data: parseSse(text) };
    }
    let json;
    try { json = JSON.parse(text); } catch { check(false, 'invalid-api-json'); }
    return { status: response.status, json };
  }
  async call(route, options) {
    const result = await this.raw(route, options);
    check(result.status >= 200 && result.status < 300 && result.json?.ok === true
      && Object.hasOwn(result.json, 'data'), 'api-request-refused');
    return result.json.data;
  }
}
export function publicReport({ phase, passed, quiesced, assertions, failure }) {
  return {
    schema_version: 1, kind: 'hosted-preview-candidate-lifecycle',
    product_source: PRODUCT_SOURCE, stable_source: STABLE_SOURCE,
    phase, executed: true, passed: Boolean(passed), processes_quiesced: Boolean(quiesced),
    assertions: assertions.map(item => ({
      name: /^[a-z0-9-]+$/.test(item.name) ? item.name : 'invalid-assertion-name',
      passed: Boolean(item.passed),
    })),
    ...(failure ? { failure: /^[a-z0-9-]+$/.test(failure) ? failure : 'unclassified-failure' } : {}),
    limitations: LIMITATIONS,
  };
}
