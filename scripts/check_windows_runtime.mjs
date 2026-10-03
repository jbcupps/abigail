#!/usr/bin/env node
// Inspect bytes only. This tool never loads, starts or extracts an executable.
import { createHash } from 'node:crypto';
import { closeSync, existsSync, fstatSync, openSync, readSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const MAX_EXECUTABLE_BYTES = 256 * 1024 * 1024;
const MAX_SECTIONS = 96;
const MAX_IMPORTS = 256;
const MAX_DIRECTORY_BYTES = 64 * 1024;
const MAX_DLL_NAME_BYTES = 260;
const UINT32_END = 0x1_0000_0000;

export class PeInspectionError extends Error {
  constructor(message) {
    super(message);
    this.name = 'PeInspectionError';
    this.code = 'INVALID_WINDOWS_EXECUTABLE';
  }
}

function requireCondition(condition, message) {
  if (!condition) throw new PeInspectionError(message);
}

export function requiresExternalVcRuntime(name) {
  const lower = name.toLowerCase();
  if (lower === 'msvcrt.dll' || lower === 'msvcp_win.dll' || lower === 'ucrtbase.dll' || lower.startsWith('api-ms-win-crt-')) return false;
  return /^(?:vcruntime|msvcp|concrt|msvcr|vcomp)[a-z0-9_-]*\.dll$/i.test(name) || lower === 'ucrtbased.dll';
}

export function inspectPe32Plus(bytes) {
  requireCondition(Buffer.isBuffer(bytes) && bytes.length <= MAX_EXECUTABLE_BYTES, 'Executable exceeds the bounded PE input size.');
  function span(offset, length, label) {
    requireCondition(Number.isSafeInteger(offset) && Number.isSafeInteger(length) && offset >= 0 && length >= 0 &&
      offset <= bytes.length && length <= bytes.length - offset, `${label} lies outside the executable bytes.`);
  }
  function u16(offset, label) { span(offset, 2, label); return bytes.readUInt16LE(offset); }
  function u32(offset, label) { span(offset, 4, label); return bytes.readUInt32LE(offset); }
  span(0, 64, 'DOS header');
  requireCondition(bytes.readUInt16LE(0) === 0x5a4d, 'Executable has no DOS MZ header.');
  const pe = u32(0x3c, 'PE header pointer');
  requireCondition(pe >= 64 && pe <= 1024 * 1024, 'PE header pointer exceeds the bounded header region.');
  span(pe, 24, 'PE/COFF header');
  requireCondition(bytes.readUInt32LE(pe) === 0x00004550, 'Executable has no PE signature.');
  requireCondition(u16(pe + 4, 'Machine') === 0x8664, 'Executable must target Windows x64 (AMD64).');
  const sectionCount = u16(pe + 6, 'Section count');
  requireCondition(sectionCount > 0 && sectionCount <= MAX_SECTIONS, 'PE section count exceeds its supported bounds.');
  const characteristics = u16(pe + 22, 'COFF characteristics');
  requireCondition((characteristics & 0x0002) !== 0 && (characteristics & 0x2000) === 0, 'PE input must be an executable image, not a DLL.');
  const optionalSize = u16(pe + 20, 'Optional header size');
  requireCondition(optionalSize >= 112 && optionalSize <= 4096, 'PE optional header size is invalid.');
  const optional = pe + 24;
  span(optional, optionalSize, 'Optional header');
  requireCondition(u16(optional, 'Optional header magic') === 0x20b, 'Executable must use the x64 PE32+ format.');
  const headerSize = u32(optional + 60, 'SizeOfHeaders');
  const imageSize = u32(optional + 56, 'SizeOfImage');
  const directoryCount = u32(optional + 108, 'Data directory count');
  requireCondition(directoryCount <= 16 && 112 + directoryCount * 8 <= optionalSize, 'PE data directories exceed the optional header.');
  const sectionTable = optional + optionalSize;
  span(sectionTable, sectionCount * 40, 'Section table');
  requireCondition(headerSize >= sectionTable + sectionCount * 40 && headerSize <= bytes.length && imageSize >= headerSize,
    'PE header/image size cannot contain its declared tables.');
  const imageBase = bytes.readBigUInt64LE(optional + 24);
  const sections = [];
  for (let index = 0; index < sectionCount; index++) {
    const entry = sectionTable + index * 40;
    const virtualSize = u32(entry + 8, 'Section virtual size');
    const virtualAddress = u32(entry + 12, 'Section virtual address');
    const rawSize = u32(entry + 16, 'Section raw size');
    const rawAddress = u32(entry + 20, 'Section raw address');
    const mappedSize = Math.max(virtualSize, rawSize);
    requireCondition(mappedSize > 0 && virtualAddress >= headerSize && virtualAddress + mappedSize <= UINT32_END &&
      virtualAddress + mappedSize <= imageSize, 'Section virtual range is invalid.');
    if (rawSize !== 0) {
      requireCondition(rawAddress >= headerSize, 'Section file data overlaps the PE headers.');
      span(rawAddress, rawSize, 'Section file data');
    }
    sections.push({ virtualAddress, mappedSize, rawAddress, rawSize });
  }
  function rejectOverlaps(ranges, label) {
    ranges.sort((left, right) => left.start - right.start);
    for (let index = 1; index < ranges.length; index++) {
      requireCondition(ranges[index].start >= ranges[index - 1].end, `${label} overlap and make PE addresses ambiguous.`);
    }
  }
  rejectOverlaps(sections.map(section => ({ start: section.virtualAddress, end: section.virtualAddress + section.mappedSize })), 'Section virtual ranges');
  rejectOverlaps(sections.filter(section => section.rawSize).map(section => ({ start: section.rawAddress, end: section.rawAddress + section.rawSize })), 'Section file ranges');

  function mapRva(rva, length, label) {
    requireCondition(Number.isInteger(rva) && rva > 0 && rva < UINT32_END && length > 0 && rva + length <= UINT32_END,
      `${label} has an invalid RVA range.`);
    if (rva < headerSize) {
      requireCondition(rva + length <= headerSize, `${label} crosses the header mapping.`);
      span(rva, length, label);
      return { offset: rva, available: headerSize - rva };
    }
    const matches = sections.filter(section => rva >= section.virtualAddress && rva + length <= section.virtualAddress + section.mappedSize);
    requireCondition(matches.length === 1, `${label} does not map uniquely to a PE section.`);
    const section = matches[0];
    const delta = rva - section.virtualAddress;
    requireCondition(delta + length <= section.rawSize, `${label} points to section data absent from the file.`);
    const offset = section.rawAddress + delta;
    span(offset, length, label);
    return { offset, available: section.rawSize - delta };
  }

  function dllName(rva) {
    const mapped = mapRva(rva, 1, 'DLL import name');
    const end = Math.min(MAX_DLL_NAME_BYTES, mapped.available);
    const nameBytes = bytes.subarray(mapped.offset, mapped.offset + end);
    const terminator = nameBytes.indexOf(0);
    requireCondition(terminator > 0, 'DLL import name is empty, unterminated or exceeds its bounds.');
    const value = nameBytes.subarray(0, terminator);
    requireCondition([...value].every(byte => byte >= 0x21 && byte <= 0x7e), 'DLL import name is not bounded ASCII.');
    const name = value.toString('ascii');
    requireCondition(/^[A-Za-z0-9_][A-Za-z0-9_.-]*\.dll$/i.test(name), 'DLL import name is not a plain DLL filename.');
    return name;
  }

  function directory(index, stride, label) {
    if (index >= directoryCount) return null;
    const entry = optional + 112 + index * 8;
    const rva = u32(entry, `${label} RVA`);
    const size = u32(entry + 4, `${label} size`);
    requireCondition((rva === 0) === (size === 0), `${label} must be entirely absent or have both RVA and size.`);
    if (rva === 0) return null;
    requireCondition(size >= stride && size <= MAX_DIRECTORY_BYTES, `${label} size exceeds its supported bounds.`);
    return { offset: mapRva(rva, size, label).offset, size };
  }
  function thunkPointer(rva, label) {
    requireCondition(rva !== 0 && rva % 8 === 0, `${label} must have an aligned nonzero RVA.`);
    mapRva(rva, 8, label);
  }
  function readImports(delay) {
    const label = delay ? 'Delay import directory' : 'Import directory';
    const stride = delay ? 32 : 20;
    const table = directory(delay ? 13 : 1, stride, label);
    if (!table) return [];
    const result = [];
    for (let index = 0; index <= MAX_IMPORTS; index++) {
      requireCondition((index + 1) * stride <= table.size, `${label} has no complete terminator within its declared size.`);
      const entry = table.offset + index * stride;
      const fields = Array.from({ length: stride / 4 }, (_, field) => u32(entry + field * 4, label));
      if (fields.every(field => field === 0)) return result;
      requireCondition(index < MAX_IMPORTS, `${label} exceeds its descriptor limit.`);
      if (delay) {
        requireCondition(fields[0] === 0 || fields[0] === 1, 'Delay import attributes contain unsupported flags.');
        function address(value, addressLabel) {
          if (fields[0] === 1) return value;
          requireCondition(BigInt(value) >= imageBase, `${addressLabel} VA lies below the image base.`);
          const rva = BigInt(value) - imageBase;
          requireCondition(rva < BigInt(UINT32_END), `${addressLabel} VA exceeds the supported image range.`);
          return Number(rva);
        }
        result.push(dllName(address(fields[1], 'Delay import name')));
        thunkPointer(address(fields[3], 'Delay import IAT'), 'Delay import IAT');
        thunkPointer(address(fields[4], 'Delay import INT'), 'Delay import INT');
        for (const field of [2, 5, 6]) {
          if (fields[field] !== 0) mapRva(address(fields[field], 'Delay import pointer'), 8, 'Delay import pointer');
        }
      } else {
        result.push(dllName(fields[3]));
        thunkPointer(fields[4], 'Import IAT');
        if (fields[0] !== 0) thunkPointer(fields[0], 'Import lookup table');
      }
    }
    throw new PeInspectionError(`${label} exceeds its descriptor limit.`);
  }
  const imports = readImports(false);
  const delayImports = readImports(true);
  return { imports, delay_imports: delayImports };
}

export function inspectWindowsExecutable(inputPath) {
  const path = realpathSync(resolve(inputPath));
  requireCondition(extname(path).toLowerCase() === '.exe', 'Runtime inspection accepts executable (.exe) paths only.');
  const descriptor = openSync(path, 'r');
  let bytes;
  try {
    const before = fstatSync(descriptor);
    requireCondition(before.isFile() && before.size > 0 && before.size <= MAX_EXECUTABLE_BYTES, 'Executable must be a nonempty regular file of at most 256 MiB.');
    bytes = Buffer.allocUnsafe(before.size);
    let offset = 0;
    while (offset < bytes.length) {
      const count = readSync(descriptor, bytes, offset, bytes.length - offset, offset);
      requireCondition(count > 0, 'Executable became truncated during inspection.');
      offset += count;
    }
    const extra = Buffer.alloc(1);
    requireCondition(readSync(descriptor, extra, 0, 1, bytes.length) === 0, 'Executable grew during inspection.');
    const after = fstatSync(descriptor);
    requireCondition(before.size === after.size && before.mtimeMs === after.mtimeMs, 'Executable changed during inspection.');
  } finally { closeSync(descriptor); }
  const parsed = inspectPe32Plus(bytes);
  const external = [...parsed.imports, ...parsed.delay_imports].filter(requiresExternalVcRuntime);
  return { path, sha256: createHash('sha256').update(bytes).digest('hex').toUpperCase(),
    imports: parsed.imports, delay_imports: parsed.delay_imports, requires_external_vc_runtime: external.length !== 0,
    ...(external.length ? { external_vc_runtime: external } : {}) };
}

export function checkWindowsRuntime(paths) {
  requireCondition(paths.length > 0 && paths.length <= 64, 'Inspect between one and sixty-four Windows executable paths.');
  const report = { schema_version: 1, passed: true, architecture: 'x64', executables: [] };
  const errors = [];
  for (const path of paths) {
    try {
      const executable = inspectWindowsExecutable(path);
      report.executables.push(executable);
      if (executable.requires_external_vc_runtime) {
        report.passed = false;
        errors.push({ path: executable.path, code: 'EXTERNAL_VC_RUNTIME', message: 'Executable requires a separately deployed Microsoft Visual C++ runtime.' });
      }
    } catch (error) {
      report.passed = false;
      errors.push({ path: resolve(path), code: error instanceof PeInspectionError ? error.code : 'EXECUTABLE_READ_FAILED',
        message: error instanceof PeInspectionError ? error.message : 'Unable to read the executable for bounded runtime inspection.' });
    }
  }
  if (errors.length) report.errors = errors;
  return report;
}

function main(args) {
  const paths = [];
  let reportPath;
  let positional = false;
  for (let index = 0; index < args.length; index++) {
    const argument = args[index];
    if (!positional && argument === '--') { positional = true; continue; }
    if (!positional && argument === '--report') {
      requireCondition(!reportPath && index + 1 < args.length && !args[index + 1].startsWith('--'), '--report requires exactly one output path.');
      reportPath = resolve(args[++index]);
    } else {
      requireCondition(positional || !argument.startsWith('-'), 'Unknown runtime checker option.');
      paths.push(argument);
    }
  }
  const report = checkWindowsRuntime(paths);
  if (reportPath) {
    const canonicalReport = existsSync(reportPath) ? realpathSync(reportPath) : join(realpathSync(dirname(reportPath)), basename(reportPath));
    const identity = value => process.platform === 'win32' ? value.toLowerCase() : value;
    requireCondition(!paths.some(path => identity(resolve(path)) === identity(canonicalReport) || (() => { try { return identity(realpathSync(resolve(path))) === identity(canonicalReport); } catch { return false; } })()),
      'Runtime report must not overwrite an executable input.');
    if (existsSync(canonicalReport)) {
      const outputIdentity = statSync(canonicalReport, { bigint: true });
      requireCondition(!paths.some(path => {
        try {
          const inputIdentity = statSync(path, { bigint: true });
          return inputIdentity.dev === outputIdentity.dev && inputIdentity.ino === outputIdentity.ino;
        } catch { return false; }
      }), 'Runtime report must not overwrite a hard-linked executable input.');
    }
    writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n', 'utf8');
  }
  if (!report.passed) {
    for (const error of report.errors) console.error(`${error.code}: ${error.path}: ${error.message}`);
    process.exitCode = 1;
  } else {
    console.log(`Windows x64 runtime inspection passed for ${report.executables.length} executable(s).`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); }
  catch (error) {
    console.error(error instanceof PeInspectionError ? error.message : 'Runtime checker could not complete its bounded inspection or report write.');
    process.exitCode = 1;
  }
}
