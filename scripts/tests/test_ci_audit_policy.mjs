import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const read = (path) => readFileSync(resolve(repositoryRoot, path), 'utf8');

function jobBlock(workflow, name) {
  const start = workflow.indexOf(`\n  ${name}:\n`);
  assert.ok(start >= 0, `workflow job ${name} exists`);
  const rest = workflow.slice(start + 1);
  const next = rest.slice(1).search(/\n  [a-z][a-z0-9_-]*:\n/);
  return next < 0 ? rest : rest.slice(0, next + 1);
}

export function parseAuditIgnores(toml) {
  const lines = toml.split(/\r?\n/);
  const start = lines.findIndex((line) => /^ignore\s*=\s*\[/.test(line.trim()));
  if (start < 0) return [];
  const entries = [];
  let comments = [];
  for (const raw of lines.slice(start + 1)) {
    const line = raw.trim();
    if (line.startsWith(']')) break;
    if (line.startsWith('#')) {
      comments.push(line.replace(/^#\s?/, ''));
      continue;
    }
    const match = line.match(/^"(RUSTSEC-\d{4}-\d{4})",?$/);
    if (match) {
      entries.push({ id: match[1], reason: comments.join(' ') });
      comments = [];
    } else if (line) {
      throw new Error(`Unexpected audit ignore entry: ${line}`);
    }
  }
  return entries;
}

test('cargo audit runs without inline ignore flags', () => {
  const audit = jobBlock(read('.github/workflows/ci.yml'), 'audit');
  assert.match(audit, /run: cargo audit\s*\n/);
  assert.doesNotMatch(audit, /--ignore/);
  assert.match(audit, /npm audit --audit-level=high/);
});

test('the CI gate treats the dependency audit as required', () => {
  const gate = jobBlock(read('.github/workflows/ci.yml'), 'gate');
  const required = gate.match(/# Critical jobs[^\n]*\n\s*for job in ([^;]+);/);
  assert.ok(required, 'gate lists its required jobs');
  assert.ok(required[1].split(/\s+/).includes('audit'), 'audit is a required gate job');
  const advisory = gate.match(/# Advisory jobs[^\n]*\n\s*for job in ([^;]+);/);
  assert.ok(advisory, 'gate lists its advisory jobs');
  assert.ok(!advisory[1].split(/\s+/).includes('audit'), 'audit is not advisory');
});

test('every audit exception names a locked crate and explains its removal condition', () => {
  const ignores = parseAuditIgnores(read('.cargo/audit.toml'));
  const lock = read('Cargo.lock');
  const ids = ignores.map((entry) => entry.id);
  assert.equal(new Set(ids).size, ids.length, 'no duplicate exceptions');
  for (const { id, reason } of ignores) {
    const crate = reason.split(/\s+/)[0];
    assert.ok(crate, `${id} has a reason`);
    assert.ok(lock.includes(`\nname = "${crate}"\n`), `${id} names ${crate}, which must still be in Cargo.lock`);
    assert.match(reason, /\bRemove when\b/i, `${id} states when to drop the exception`);
  }
});

test('the audit exception parser rejects undocumented or malformed entries', () => {
  const parsed = parseAuditIgnores('[advisories]\nignore = [\n  "RUSTSEC-2020-0001",\n]\n');
  assert.deepEqual(parsed, [{ id: 'RUSTSEC-2020-0001', reason: '' }]);
  assert.throws(() => parseAuditIgnores('ignore = [\n  RUSTSEC-2020-0001\n]\n'), /Unexpected audit ignore entry/);
});
