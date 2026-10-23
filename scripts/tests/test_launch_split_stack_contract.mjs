import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const readIfPresent = (path) => existsSync(resolve(root, path)) ? read(path) : '';
const launcher = read('scripts/dev/launch_split_stack.ps1');
const helper = readIfPresent('scripts/dev/start_process_with_environment_overlay.ps1');
const behavioralTest = readIfPresent('scripts/tests/test_launch_split_stack_environment.ps1');
const workflow = read('.github/workflows/ci.yml');
const readme = read('README.md');
const design = read('docs/superpowers/specs/2026-10-23-genesis-quest-continuation-design.md');
const plan = read('docs/superpowers/plans/2026-10-23-genesis-quest-continuation.md');

function between(source, start, end) {
  const from = source.indexOf(start);
  assert.notEqual(from, -1, `missing ${start}`);
  const to = source.indexOf(end, from + start.length);
  assert.notEqual(to, -1, `missing ${end} after ${start}`);
  return source.slice(from, to);
}

test('launcher dot-sources the isolated PowerShell 5.1 overlay helper', () => {
  assert.match(
    launcher,
    /^\.\s+\(Join-Path\s+\$PSScriptRoot\s+"start_process_with_environment_overlay\.ps1"\)$/m,
  );
  assert.doesNotMatch(launcher, /function\s+Start-ProcessWithEnvironmentOverlay\b/i);
  assert.match(helper, /function\s+Start-ProcessWithEnvironmentOverlay\b/i);
  assert.match(helper, /\[hashtable\]\s*\$EnvironmentOverlay\b/i);
  assert.match(helper, /System\.Diagnostics\.ProcessStartInfo/i);
  assert.match(helper, /\.UseShellExecute\s*=\s*\$false/i);
  assert.match(helper, /\.EnvironmentVariables\s*\[/i);
  assert.match(helper, /\[System\.Diagnostics\.Process\]::Start\s*\(/i);
  assert.match(helper, /\[string\]\s*\$Arguments\b/i);
  assert.match(helper, /\$startInfo\.Arguments\s*=/i);
  assert.doesNotMatch(helper, /\b(?:Add-Type|DllImport|SetEnvironmentVariable|GetEnvironmentVariable)\b/i);
  assert.doesNotMatch(helper, /\b(?:ArgumentList|Wait|PassThru)\b/i);
  assert.doesNotMatch(helper, /\bWrite-(?:Host|Verbose|Debug|Information|Warning|Output)\b/i);
  assert.doesNotMatch(launcher, /Start-ProcessWithEnvironmentOverlay[\s\S]*?-Arguments\b/i);
});

test('both desktop shells use the overlay helper with their required variables', () => {
  const hiveLaunch = between(
    launcher,
    '$hiveAppProc =',
    'Write-Host "Launching Abigail Entity Runtime desktop shell..."',
  );
  assert.match(hiveLaunch, /\$hiveAppProc\s*=\s*Start-ProcessWithEnvironmentOverlay\b/i);
  assert.match(hiveLaunch, /\bABIGAIL_HIVE_URL\s*=\s*\$hiveUrl\b/i);
  assert.match(hiveLaunch, /\bABIGAIL_DATA_DIR\s*=\s*\$dataRoot\b/i);
  assert.match(hiveLaunch, /\bCARGO_TARGET_DIR\s*=\s*\$targetDir\b/i);

  const runtimeLaunch = between(launcher, '$runtimeAppProc =', '} elseif (-not $NoBrowserFallback)');
  assert.match(runtimeLaunch, /\$runtimeAppProc\s*=\s*Start-ProcessWithEnvironmentOverlay\b/i);
  assert.match(runtimeLaunch, /\bABIGAIL_ENTITY_URL\s*=\s*\$runtimeUrl\b/i);
  assert.match(runtimeLaunch, /\bCARGO_TARGET_DIR\s*=\s*\$targetDir\b/i);
});

test('Windows PowerShell behavior test owns syntax and child-environment coverage', () => {
  assert.match(
    behavioralTest,
    /\[Management\.Automation\.Language\.Parser\]::ParseFile\s*\(/i,
  );
  assert.match(behavioralTest, /-EncodedCommand\b/i);
  assert.match(behavioralTest, /WaitForExit\s*\(\s*\$childWaitTimeoutMs\s*\)/i);
  assert.match(behavioralTest, /\.Kill\s*\(/i);
  assert.match(behavioralTest, /\.Dispose\s*\(/i);
  assert.match(behavioralTest, /Start-ProcessWithEnvironmentOverlay[\s\S]*?-Arguments\s+\$argumentString/i);
  assert.match(behavioralTest, /PRESENT/);
  assert.match(behavioralTest, /ABSENT/);
  assert.match(behavioralTest, /does-not-exist\.exe/);
  assert.match(behavioralTest, /^\.\s+\$helperPath$/m);
  assert.doesNotMatch(behavioralTest, /^\.\s+.*launch_split_stack\.ps1/m);
  assert.doesNotMatch(behavioralTest, /\bNODE_OPTIONS\b/);
  assert.doesNotMatch(behavioralTest, /run-child\.js/);
  assert.doesNotMatch(behavioralTest, /Get-ProcessEnvironmentVariableExact\b/);
});

test('CI runs both static wiring and Windows PowerShell 5.1 behavior contracts', () => {
  assert.match(
    workflow,
    /\bnode\s+--test\s+scripts\/tests\/test_launch_split_stack_contract\.mjs\b/,
  );
  assert.match(
    workflow,
    /\bpowershell\.exe\s+-NoProfile\s+-File\s+\.\\scripts\\tests\\test_launch_split_stack_environment\.ps1\b/i,
  );
});

test('documentation specifies a child-only ProcessStartInfo environment', () => {
  for (const [path, content] of [
    ['README.md', readme],
    ['continuation design', design],
    ['Task 7 plan', plan],
  ]) {
    assert.match(content, /ProcessStartInfo/i, `${path} names the child process mechanism`);
    assert.match(content, /(?:does not|never) (?:change|mutate|modify).*parent process environment/i,
      `${path} preserves the parent by construction`);
    assert.doesNotMatch(content, /temporar(?:y|ily) (?:set|overlay).*process environment/i,
      `${path} does not describe the retired parent-overlay design`);
  }
});
