import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, linkSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, truncateSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { checkWindowsRuntime, inspectPe32Plus, inspectWindowsExecutable, MAX_EXECUTABLE_BYTES, PeInspectionError } from '../check_windows_runtime.mjs';
import { createPeFixture } from './windows_runtime_fixtures.mjs';

const temporaryParent = realpathSync(tmpdir());
const testRoot = realpathSync(mkdtempSync(join(temporaryParent, 'abigail-windows-runtime-')));
const checker = resolve(dirname(fileURLToPath(import.meta.url)), '../check_windows_runtime.mjs');
const repositoryRoot = resolve(dirname(checker), '..');
const PE = 0x80;
const OPTIONAL = PE + 24;
const SECTION = OPTIONAL + 240;
const RAW = 0x200;
const RVA = 0x1000;
let fileCounter = 0;
after(() => {
  const canonical = realpathSync(testRoot);
  assert(canonical.startsWith(temporaryParent + sep) && basename(canonical).startsWith('abigail-windows-runtime-'));
  rmSync(canonical, { recursive: true, force: true });
});
function fixture(options) {
  const path = join(testRoot, `fixture-${fileCounter++}.exe`);
  writeFileSync(path, createPeFixture(options));
  return path;
}
function directoryEntry(index) { return OPTIONAL + 112 + index * 8; }
function tableOffset(bytes, delay = false) { return RAW + bytes.readUInt32LE(directoryEntry(delay ? 13 : 1)) - RVA; }
function firstNameOffset(bytes, delay = false) {
  const table = tableOffset(bytes, delay);
  let address = bytes.readUInt32LE(table + (delay ? 4 : 12));
  if (delay && bytes.readUInt32LE(table) === 0) address -= Number(bytes.readBigUInt64LE(OPTIONAL + 24));
  return RAW + address - RVA;
}
function malformed(mutate, pattern) {
  const bytes = createPeFixture();
  mutate(bytes);
  assert.throws(() => inspectPe32Plus(bytes), error => error instanceof PeInspectionError && pattern.test(error.message));
}
function cli(args) { return spawnSync(process.execPath, [checker, ...args], { encoding: 'utf8', timeout: 10000, windowsHide: true }); }

test('allowed Windows OS imports produce canonical path and whole-file SHA256 report', () => {
  const path = fixture({ imports: ['KERNEL32.dll', 'msvcrt.dll', 'ucrtbase.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'msvcp_win.dll'], delayImports: ['USER32.dll'] });
  const report = checkWindowsRuntime([relative(process.cwd(), path)]);
  assert.equal(report.schema_version, 1);
  assert.equal(report.passed, true);
  assert.equal(report.architecture, 'x64');
  assert.deepEqual(report.executables, [{ path: realpathSync(path), sha256: createHash('sha256').update(readFileSync(path)).digest('hex').toUpperCase(),
    imports: ['KERNEL32.dll', 'msvcrt.dll', 'ucrtbase.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'msvcp_win.dll'], delay_imports: ['USER32.dll'], requires_external_vc_runtime: false }]);
});

test('all import directories can be absent without inventing a dependency', () => {
  const bytes = createPeFixture({ imports: [], delayImports: [] });
  assert.deepEqual(inspectPe32Plus(bytes), { imports: [], delay_imports: [] });
  bytes.writeUInt32LE(0, OPTIONAL + 108);
  assert.deepEqual(inspectPe32Plus(bytes), { imports: [], delay_imports: [] });
});

test('normal redist and debug CRT imports reject an otherwise valid executable', () => {
  for (const dll of ['VCRUNTIME140.dll', 'vcruntime140_1.dll', 'VCRUNTIME140D.dll', 'MSVCP140.dll', 'MSVCP140_2.dll', 'CONCRT140.dll', 'MSVCR120.dll', 'VCOMP140.dll', 'ucrtbased.dll', 'msvcrtd.dll']) {
    const path = fixture({ imports: ['KERNEL32.dll', dll] });
    const report = checkWindowsRuntime([path]);
    assert.equal(report.passed, false, dll);
    assert.equal(report.executables[0].requires_external_vc_runtime, true, dll);
    assert.deepEqual(report.executables[0].external_vc_runtime, [dll]);
    assert.equal(report.errors[0].code, 'EXTERNAL_VC_RUNTIME');
  }
});

test('a runtime present only in delay imports is also rejected', () => {
  const path = fixture({ imports: ['KERNEL32.dll'], delayImports: ['VCRUNTIME140_1.dll'] });
  const report = checkWindowsRuntime([path]);
  assert.equal(report.passed, false);
  assert.deepEqual(report.executables[0].imports, ['KERNEL32.dll']);
  assert.deepEqual(report.executables[0].delay_imports, ['VCRUNTIME140_1.dll']);
  assert.deepEqual(report.executables[0].external_vc_runtime, ['VCRUNTIME140_1.dll']);
});

test('legacy delay-import VAs map correctly and cannot conceal a runtime dependency', () => {
  const path = fixture({ imports: [], delayImports: ['VCRUNTIME140.dll'], delayUsesVa: true });
  const report = checkWindowsRuntime([path]);
  assert.equal(report.passed, false);
  assert.deepEqual(report.executables[0].delay_imports, ['VCRUNTIME140.dll']);
  const allowed = createPeFixture({ imports: [], delayImports: ['KERNEL32.dll'], delayUsesVa: true });
  assert.deepEqual(inspectPe32Plus(allowed).delay_imports, ['KERNEL32.dll']);
});

test('truncation, architecture and optional-header mismatches fail closed', () => {
  assert.throws(() => inspectPe32Plus(Buffer.from('MZ')), PeInspectionError);
  assert.throws(() => inspectPe32Plus(createPeFixture().subarray(0, 0x100)), PeInspectionError);
  malformed(bytes => bytes.writeUInt32LE(0xfffffff0, 0x3c), /PE header pointer/);
  malformed(bytes => bytes.writeUInt16LE(0x14c, PE + 4), /x64/);
  malformed(bytes => bytes.writeUInt16LE(0x10b, OPTIONAL), /PE32\+/);
  malformed(bytes => bytes.writeUInt16LE(111, PE + 20), /optional header/);
  malformed(bytes => bytes.writeUInt32LE(17, OPTIONAL + 108), /data directories/);
  malformed(bytes => bytes.writeUInt32LE(0x100, OPTIONAL + 60), /header\/image size/);
  malformed(bytes => bytes.writeUInt16LE(0x2002, PE + 22), /not a DLL/);
});

test('section file bounds and non-unique virtual/file ranges fail closed', () => {
  malformed(bytes => bytes.writeUInt32LE(bytes.length, SECTION + 20), /Section file data/);
  malformed(bytes => bytes.writeUInt16LE(97, PE + 6), /section count/);
  malformed(bytes => {
    bytes.writeUInt16LE(2, PE + 6);
    bytes.writeUInt32LE(1, SECTION + 40 + 8);
    bytes.writeUInt32LE(RVA, SECTION + 40 + 12);
  }, /virtual ranges.*overlap/);
  malformed(bytes => {
    bytes.writeUInt16LE(2, PE + 6);
    bytes.writeUInt32LE(0x200, SECTION + 40 + 8);
    bytes.writeUInt32LE(0x11000, SECTION + 40 + 12);
    bytes.writeUInt32LE(0x200, SECTION + 40 + 16);
    bytes.writeUInt32LE(RAW, SECTION + 40 + 20);
  }, /file ranges.*overlap/);
});

test('normal/delay directory zero semantics, declared spans and termination are enforced', () => {
  for (const index of [1, 13]) {
    const options = index === 1 ? {} : { imports: [], delayImports: ['KERNEL32.dll'] };
    const zeroSize = createPeFixture(options);
    zeroSize.writeUInt32LE(0, directoryEntry(index) + 4);
    assert.throws(() => inspectPe32Plus(zeroSize), /entirely absent/);
    const zeroRva = createPeFixture(options);
    zeroRva.writeUInt32LE(0, directoryEntry(index));
    assert.throws(() => inspectPe32Plus(zeroRva), /entirely absent/);
    const missingTerminator = createPeFixture(options);
    missingTerminator.writeUInt32LE(index === 1 ? 20 : 32, directoryEntry(index) + 4);
    assert.throws(() => inspectPe32Plus(missingTerminator), /no complete terminator/);
    const overflow = createPeFixture(options);
    overflow.writeUInt32LE(0xfffffff0, directoryEntry(index));
    assert.throws(() => inspectPe32Plus(overflow), /invalid RVA range/);
  }
  malformed(bytes => bytes.writeUInt32LE(65537, directoryEntry(1) + 4), /directory.*bounds/i);
  const crowded = createPeFixture({ imports: Array(257).fill('KERNEL32.dll') });
  assert.throws(() => inspectPe32Plus(crowded), /descriptor limit/);
});

test('names must be terminated inside mapped file bytes and be plain ASCII DLL names', () => {
  malformed(bytes => bytes.fill(0x41, firstNameOffset(bytes), firstNameOffset(bytes) + 260), /unterminated/);
  malformed(bytes => bytes[firstNameOffset(bytes)] = 0, /empty/);
  malformed(bytes => bytes[firstNameOffset(bytes)] = 0x80, /ASCII/);
  malformed(bytes => bytes.write('../private.dll\0', firstNameOffset(bytes), 'ascii'), /plain DLL/);
  malformed(bytes => bytes.writeUInt32LE(0, tableOffset(bytes) + 12), /invalid RVA/);
  malformed(bytes => {
    bytes.writeUInt32LE(bytes.length - RAW + 0x1000, SECTION + 8);
    bytes.writeUInt32LE(RVA + bytes.length - RAW, tableOffset(bytes) + 12);
  }, /absent from the file/);
  malformed(bytes => {
    bytes.fill(0x41, bytes.length - 2);
    bytes.writeUInt32LE(RVA + bytes.length - RAW - 2, tableOffset(bytes) + 12);
  }, /unterminated/);
});

test('delay attributes, VA conversion and IAT/INT pointers fail closed when invalid', () => {
  const options = { imports: [], delayImports: ['KERNEL32.dll'] };
  const attributes = createPeFixture(options);
  attributes.writeUInt32LE(2, tableOffset(attributes, true));
  assert.throws(() => inspectPe32Plus(attributes), /unsupported flags/);
  const wrongVa = createPeFixture(options);
  wrongVa.writeUInt32LE(0, tableOffset(wrongVa, true));
  assert.throws(() => inspectPe32Plus(wrongVa), /below the image base/);
  for (const pointerOffset of [12, 16]) {
    const missing = createPeFixture(options);
    missing.writeUInt32LE(0, tableOffset(missing, true) + pointerOffset);
    assert.throws(() => inspectPe32Plus(missing), /aligned nonzero RVA/);
  }
  malformed(bytes => bytes.writeUInt32LE(0xfffffff8, tableOffset(bytes) + 16), /does not map uniquely/);
});

test('oversized files and non-executable paths are rejected before PE parsing', () => {
  const oversized = join(testRoot, 'oversized.exe');
  writeFileSync(oversized, Buffer.alloc(0));
  truncateSync(oversized, MAX_EXECUTABLE_BYTES + 1);
  assert.throws(() => inspectWindowsExecutable(oversized), /at most 256 MiB/);
  const text = join(testRoot, 'not-an-executable.txt');
  writeFileSync(text, 'synthetic');
  assert.throws(() => inspectWindowsExecutable(text), /executable \(\.exe\) paths/);
  const directory = join(testRoot, 'directory.exe');
  mkdirSync(directory);
  const report = checkWindowsRuntime([directory]);
  assert.equal(report.passed, false);
  assert.equal(report.executables.length, 0);
});

test('CLI writes required success/failure schema and rejects missing/unknown arguments', () => {
  const allowed = fixture({ imports: ['KERNEL32.dll'], delayImports: ['ucrtbase.dll'] });
  const path = join(testRoot, 'success.json');
  assert.equal(cli(['--report', path, allowed]).status, 0);
  const report = JSON.parse(readFileSync(path, 'utf8'));
  assert.equal(report.passed, true);
  assert.equal(report.executables[0].requires_external_vc_runtime, false);
  const denied = fixture({ delayImports: ['VCRUNTIME140_1.dll'] });
  const failedPath = join(testRoot, 'failure.json');
  assert.equal(cli(['--report', failedPath, allowed, denied]).status, 1);
  const failed = JSON.parse(readFileSync(failedPath, 'utf8'));
  assert.equal(failed.passed, false);
  assert.equal(failed.executables.length, 2);
  assert.equal(failed.executables[1].requires_external_vc_runtime, true);
  assert.equal(cli([]).status, 1);
  assert.equal(cli(['--report']).status, 1);
  assert.equal(cli(['--unknown', allowed]).status, 1);
});

test('CLI report cannot overwrite an executable by path, case alias, symlink or hard link', () => {
  const path = fixture();
  const original = readFileSync(path);
  assert.equal(cli(['--report', path, path]).status, 1);
  assert.deepEqual(readFileSync(path), original);
  if (process.platform === 'win32') {
    assert.equal(cli(['--report', path.toUpperCase(), path]).status, 1);
    assert.deepEqual(readFileSync(path), original);
  }
  const alias = join(testRoot, 'input-alias.json');
  let symlinkCreated = false;
  try { symlinkSync(path, alias); symlinkCreated = true; }
  catch (error) { if (error.code !== 'EPERM' && error.code !== 'EACCES') throw error; }
  if (symlinkCreated) assert.equal(cli(['--report', alias, path]).status, 1);
  assert.deepEqual(readFileSync(path), original);
  const hardLink = join(testRoot, 'input-hardlink.json');
  linkSync(path, hardLink);
  assert.equal(cli(['--report', hardLink, path]).status, 1);
  assert.deepEqual(readFileSync(path), original);
});

const knownBadCandidate = join(repositoryRoot, 'target/manual-test/startup-fixed-candidate-validation/runs/20261002T235134Z-ce1e5ee7/installed/Abigail.exe');
test('exact previously accepted signed candidate is rejected for its actual external CRT imports',
  { skip: !existsSync(knownBadCandidate) ? 'The local signed acceptance artifact is not present on this runner.' : false }, () => {
    const executable = inspectWindowsExecutable(knownBadCandidate);
    assert.equal(executable.sha256, '950D0FB4D902A5D0A97AAFBDD0CB76A011663F074F73DC607343206D4123BC28');
    assert.equal(executable.requires_external_vc_runtime, true);
    assert(executable.imports.includes('VCRUNTIME140.dll'));
    assert(executable.imports.includes('VCRUNTIME140_1.dll'));
    const report = checkWindowsRuntime([knownBadCandidate]);
    assert.equal(report.passed, false);
    assert.equal(report.errors[0].code, 'EXTERNAL_VC_RUNTIME');
  });
