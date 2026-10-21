// Keeps the preview workflow, packaging scripts, Tauri overlays, NSIS hooks,
// and the Rust channel identity consistent. Runs on Linux without Windows.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const read = (path) => readFileSync(join(root, path), 'utf8');
const json = (path) => JSON.parse(read(path));

/** Parameter names declared in a PowerShell script's top-level param() block. */
export function declaredParameters(script) {
  const start = script.search(/^param\s*\(/im);
  if (start < 0) return new Set();
  let depth = 0;
  let end = start;
  for (let i = script.indexOf('(', start); i < script.length; i++) {
    if (script[i] === '(') depth++;
    if (script[i] === ')' && --depth === 0) { end = i; break; }
  }
  return new Set([...script.slice(start, end).matchAll(/\$(\w+)/g)].map((m) => m[1].toLowerCase()));
}

function nsisDefines(source) {
  return Object.fromEntries([...source.matchAll(/^!define\s+(\w+)\s+"([^"]*)"/gm)].map((m) => [m[1], m[2]]));
}

const workflow = read('.github/workflows/preview-candidate.yml');
const hiveBase = json('hive-app/tauri.conf.json');
const runtimeBase = json('entity-runtime-app/tauri.conf.json');
const hivePreview = json('hive-app/tauri.preview.conf.json');
const runtimePreview = json('entity-runtime-app/tauri.preview.conf.json');

test('preview workflow runs only in-tree scripts with parameters they declare', () => {
  const invocations = [...workflow.matchAll(/\.\/(scripts\/[\w./-]+\.ps1)([^\n]*)/g)];
  assert.ok(invocations.length > 0, 'workflow invokes a packaging script');
  for (const [, script, args] of invocations) {
    assert.ok(existsSync(join(root, script)), `${script} exists`);
    const declared = declaredParameters(read(script));
    for (const [, name] of args.matchAll(/(?:^|\s)-(\w+)/g)) {
      assert.ok(declared.has(name.toLowerCase()), `${script} declares -${name}`);
    }
  }
  assert.match(workflow, /build-mvp-windows\.ps1 -Channel preview\b/);
  assert.ok(declaredParameters(read('scripts/build-mvp-windows.ps1')).has('channel'));
  for (const [, test] of workflow.matchAll(/node --test (\S+)/g)) {
    assert.ok(existsSync(join(root, test)), `${test} exists`);
  }
  for (const [, path] of workflow.matchAll(/^\s+(dist\/[^\s]+)$/gm)) {
    assert.ok(path.startsWith('dist/windows-preview/'), `${path} is a preview output`);
  }
});

test('preview workflow builds this commit without credentials', () => {
  assert.doesNotMatch(workflow, /secrets\./, 'no secrets are referenced');
  assert.doesNotMatch(workflow, /^\s+ref:/m, 'checkout uses the dispatched commit');
  assert.match(workflow, /^permissions:\n\s+contents: read$/m);
  assert.match(workflow, /persist-credentials: false/);
  assert.doesNotMatch(workflow, /gh release|softprops|upload-release|sign_windows|esigner/i);
  const toolchain = read('rust-toolchain.toml').match(/^channel\s*=\s*"([^"]+)"/m)[1];
  const pins = [...workflow.matchAll(/^\s+toolchain:\s*(\S+)/gm)].map((m) => m[1]);
  assert.deepEqual(pins, [toolchain], 'Rust matches the repository toolchain');
  for (const [, ref] of workflow.matchAll(/uses:\s*\S+@(\S+)/g)) {
    assert.match(ref, /^[0-9a-f]{40}$/, 'actions are pinned to commits');
  }
});

test('preview overlays give both shells their own identity and keep stable window settings', () => {
  assert.equal(hiveBase.identifier, 'com.abigail', 'stable Hive identity is unchanged');
  assert.equal(runtimeBase.identifier, 'com.abigail.entity-runtime', 'stable Runtime identity is unchanged');
  for (const [preview, base] of [[hivePreview, hiveBase], [runtimePreview, runtimeBase]]) {
    assert.ok(preview.identifier.startsWith('com.abigail.preview'));
    assert.notEqual(preview.identifier, base.identifier);
    assert.notEqual(preview.productName, base.productName);
    assert.equal(preview.app.windows.length, base.app.windows.length);
    preview.app.windows.forEach((window, index) => {
      const { title, ...settings } = window;
      const { title: baseTitle, ...baseSettings } = base.app.windows[index];
      assert.deepEqual(settings, baseSettings, 'only the window title differs');
      assert.notEqual(title, baseTitle);
    });
  }
  assert.notEqual(hivePreview.identifier, runtimePreview.identifier);
  assert.ok(hivePreview.mainBinaryName && hivePreview.mainBinaryName !== 'Abigail');
  const hooks = resolve(root, 'hive-app', hivePreview.bundle.windows.nsis.installerHooks);
  assert.equal(hooks, join(root, 'scripts/nsis-preview-hooks.nsh'));
});

test('preview NSIS hooks guard the preview launcher and registry keys', () => {
  const preview = read('scripts/nsis-preview-hooks.nsh');
  const defines = nsisDefines(preview);
  assert.equal(defines.ABIGAIL_PRODUCT_NAME, hivePreview.productName);
  assert.equal(defines.ABIGAIL_MAIN_EXECUTABLE, `${hivePreview.mainBinaryName}.exe`);
  assert.match(preview, /!include "\$\{__FILEDIR__\}\/nsis-mvp-hooks\.nsh"/);
  const shared = read('scripts/nsis-mvp-hooks.nsh');
  assert.equal(shared.split('Abigail.exe').length - 1, 1, 'Abigail.exe appears only as the stable default');
  assert.doesNotMatch(shared, /Uninstall\\Abigail"|"Software\\abigail\\Abigail"/, 'registry keys follow the product name');
  assert.doesNotMatch(shared, /StrCmp \$R3 "Abigail"/, 'display-name match follows the product name');
});

test('packaging identity matches the Rust channel crate', () => {
  const channel = read('crates/abigail-channel/src/lib.rs');
  const build = read('crates/abigail-channel/build.rs');
  const packaging = read('scripts/packaging_channel.ps1');
  assert.match(build, /abigail-build-channel=\{channel\};/);
  assert.match(packaging, /BuildMarker = "abigail-build-channel=\$Channel;"/);
  const productNames = channel.match(/fn product_name[\s\S]*?Self::Preview => "([^"]+)"/)[1];
  assert.equal(productNames, hivePreview.productName);
  const keyring = channel.match(/fn keyring_service[\s\S]*?Self::Preview => "([^"]+)"/)[1];
  assert.ok(!keyring.includes('com.abigail.preview'), 'only preview app binaries may embed the preview identifier');
  for (const [, literal] of channel.matchAll(/"(abigail-build-channel=[^"]*)"/g)) {
    assert.ok(!/=(stable|preview);/.test(literal), `lib.rs must not spell out a full marker: ${literal}`);
  }
});

function findMakensis() {
  if (process.env.MAKENSIS) return process.env.MAKENSIS;
  const probe = spawnSync(process.platform === 'win32' ? 'where' : 'which', ['makensis'], { encoding: 'utf8' });
  return probe.status === 0 ? probe.stdout.split(/\r?\n/)[0].trim() : null;
}

const makensis = findMakensis();
const requireMakensis = process.env.ABIGAIL_REQUIRE_MAKENSIS === '1';

test('NSIS hooks compile per channel and reject a mismatched product', {
  skip: !makensis && !requireMakensis ? 'makensis is not installed' : false,
}, () => {
  assert.ok(makensis, 'ABIGAIL_REQUIRE_MAKENSIS=1 requires makensis');
  const dir = mkdtempSync(join(tmpdir(), 'abigail-nsis-'));
  try {
    const compile = (name, hooks, product, binary) => {
      const script = join(dir, `${name}.nsi`);
      writeFileSync(script, [
        'Unicode true', 'RequestExecutionLevel user', '!include LogicLib.nsh', '!include FileFunc.nsh',
        `!include "${join(root, 'scripts', hooks)}"`,
        `!define PRODUCTNAME "${product}"`, `!define MAINBINARYNAME "${binary}"`,
        'Name "fixture"', `OutFile "${join(dir, `${name}.exe`)}"`, 'InstallDir "$TEMP\\fixture"', 'Page instfiles',
        'Section "Install"', '  !insertmacro NSIS_HOOK_PREINSTALL', '  WriteUninstaller "$INSTDIR\\u.exe"', 'SectionEnd',
        'Section "Uninstall"', '  !insertmacro NSIS_HOOK_PREUNINSTALL', 'SectionEnd', '',
      ].join('\n'));
      return spawnSync(makensis, ['-V4', script], { encoding: 'utf8' });
    };
    const stable = compile('stable', 'nsis-mvp-hooks.nsh', 'Abigail', 'Abigail');
    assert.equal(stable.status, 0, stable.stdout + stable.stderr);
    assert.match(stable.stdout, /Push: \$0\\Abigail\.exe/);
    const preview = compile('preview', 'nsis-preview-hooks.nsh', hivePreview.productName, hivePreview.mainBinaryName);
    assert.equal(preview.status, 0, preview.stdout + preview.stderr);
    assert.match(preview.stdout, /Push: \$0\\Abigail Preview\.exe/);
    assert.match(preview.stdout, /Uninstall\\Abigail Preview\\InstallLocation/);
    assert.doesNotMatch(preview.stdout, /Push: \$0\\Abigail\.exe|Uninstall\\Abigail\\InstallLocation/);
    const mismatch = compile('mismatch', 'nsis-preview-hooks.nsh', 'Abigail', 'Abigail');
    assert.notEqual(mismatch.status, 0, 'preview hooks must refuse a stable installer');
    assert.match(mismatch.stdout + mismatch.stderr, /differs from hook product/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('stable lanes keep the stable NSIS hooks', () => {
  for (const path of ['.github/workflows/release.yml', 'scripts/build_signed_installer.ps1']) {
    const content = read(path);
    assert.ok(content.includes('nsis-mvp-hooks.nsh'), `${path} uses the stable hooks`);
    assert.ok(!content.includes('nsis-preview-hooks.nsh'), `${path} never uses preview hooks`);
  }
});
