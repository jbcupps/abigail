import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const launcher = readFileSync(resolve(root, 'scripts/dev/launch_split_stack.ps1'), 'utf8');
const workflow = readFileSync(resolve(root, '.github/workflows/ci.yml'), 'utf8');

function balancedBlock(source, declaration) {
  const match = declaration.exec(source);
  assert.ok(match, `missing ${declaration}`);
  const open = source.indexOf('{', match.index);
  assert.notEqual(open, -1, `missing block for ${declaration}`);
  let depth = 0;
  for (let index = open; index < source.length; index++) {
    if (source[index] === '{') depth++;
    if (source[index] === '}' && --depth === 0) return source.slice(open + 1, index);
  }
  assert.fail(`unterminated block for ${declaration}`);
}

function between(source, start, end) {
  const from = source.indexOf(start);
  assert.notEqual(from, -1, `missing ${start}`);
  const to = source.indexOf(end, from + start.length);
  assert.notEqual(to, -1, `missing ${end} after ${start}`);
  return source.slice(from, to);
}

test('PowerShell 5.1 launcher overlays child environments and restores exact parent state', () => {
  const helper = balancedBlock(
    launcher,
    /function\s+Start-ProcessWithEnvironmentOverlay\s*/i,
  );
  assert.match(helper, /\[hashtable\]\s*\$EnvironmentOverlay\b/i);

  const snapshot = helper.search(/\b(?:Test-Path|Get-Item)\b[\s\S]*?Env:/i);
  const value = helper.search(/\[Environment\]::GetEnvironmentVariable\s*\(/i);
  const overlay = helper.search(/\[Environment\]::SetEnvironmentVariable\s*\(/i);
  assert.ok(snapshot >= 0, 'helper records whether each process variable was present');
  assert.ok(value >= 0, 'helper records each prior process variable value');
  assert.ok(snapshot < overlay && value < overlay, 'helper snapshots state before applying the overlay');

  const tryBlock = balancedBlock(helper, /\btry\s*/i);
  const finallyBlock = balancedBlock(helper, /\bfinally\s*/i);
  assert.match(tryBlock, /\bStart-Process\b/i, 'spawn occurs inside try so finally runs on failure');
  assert.match(finallyBlock, /\bWasPresent\b/i, 'restore distinguishes present and absent variables');
  assert.match(
    finallyBlock,
    /\[Environment\]::SetEnvironmentVariable\s*\(/i,
    'previously present values are restored',
  );
  assert.match(
    finallyBlock,
    /\$environmentPath\s*=\s*"Env:\$name"/i,
    'restore addresses the process environment provider',
  );
  assert.match(
    finallyBlock,
    /\bRemove-Item\b[^\r\n]*-LiteralPath\s+\$environmentPath\b/i,
    'previously absent variables are removed instead of being left empty',
  );
  assert.doesNotMatch(helper, /\bWrite-(?:Host|Verbose|Debug|Information|Warning|Output)\b/i);

  for (const statement of launcher.matchAll(/\bStart-Process\b[\s\S]*?(?=\r?\n(?!\s*[`-])|$)/gi)) {
    assert.doesNotMatch(
      statement[0],
      /(?:^|\s)-Environment(?:\s|$)/i,
      'Start-Process -Environment is unavailable in Windows PowerShell 5.1',
    );
  }
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

test('CI runs the split-stack launcher contract', () => {
  assert.match(
    workflow,
    /\bnode\s+--test\s+scripts\/tests\/test_launch_split_stack_contract\.mjs\b/,
  );
});
