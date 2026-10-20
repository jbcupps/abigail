// Tiny deterministic PE64 fixtures for byte-inspection tests, never execution.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function createPeFixture({ imports = ['KERNEL32.dll'], delayImports = [], delayUsesVa = false } = {}) {
  if (imports.length + delayImports.length > 512) throw new Error('Fixture import count exceeds its bounded capacity.');
  const file = Buffer.alloc(64 * 1024);
  const pe = 0x80;
  const optional = pe + 24;
  const section = optional + 240;
  const sectionRva = 0x1000;
  const sectionRaw = 0x200;
  const imageBase = delayUsesVa ? 0x10000000n : 0x140000000n;
  file.write('MZ', 0, 'ascii');
  file.writeUInt32LE(pe, 0x3c);
  file.writeUInt32LE(0x00004550, pe);
  file.writeUInt16LE(0x8664, pe + 4);
  file.writeUInt16LE(1, pe + 6);
  file.writeUInt16LE(240, pe + 20);
  file.writeUInt16LE(0x0022, pe + 22);
  file.writeUInt16LE(0x20b, optional);
  file.writeBigUInt64LE(imageBase, optional + 24);
  file.writeUInt32LE(0x1000, optional + 32);
  file.writeUInt32LE(0x200, optional + 36);
  file.writeUInt16LE(6, optional + 40);
  file.writeUInt16LE(6, optional + 48);
  file.writeUInt32LE(0x20000, optional + 56);
  file.writeUInt32LE(0x200, optional + 60);
  file.writeUInt16LE(3, optional + 68);
  file.writeUInt32LE(16, optional + 108);
  file.write('.rdata', section, 'ascii');
  file.writeUInt32LE(file.length - sectionRaw, section + 8);
  file.writeUInt32LE(sectionRva, section + 12);
  file.writeUInt32LE(file.length - sectionRaw, section + 16);
  file.writeUInt32LE(sectionRaw, section + 20);
  file.writeUInt32LE(0x40000040, section + 36);
  let cursor = sectionRaw;
  function allocate(size, alignment = 1) {
    cursor = Math.ceil(cursor / alignment) * alignment;
    const offset = cursor;
    cursor += size;
    if (cursor > file.length) throw new Error('Fixture byte capacity exceeded.');
    return { offset, rva: sectionRva + offset - sectionRaw };
  }
  function name(value) {
    if (!/^[A-Za-z0-9_][A-Za-z0-9_.-]*\.dll$/i.test(value)) throw new Error('Fixture DLL must use a plain ASCII filename.');
    const allocation = allocate(Buffer.byteLength(value) + 1);
    file.write(value, allocation.offset, 'ascii');
    return allocation;
  }
  function thunks() {
    const allocation = allocate(16, 8);
    file.writeBigUInt64LE(0x8000000000000001n, allocation.offset);
    return allocation;
  }
  function directory(index, table) {
    const entry = optional + 112 + index * 8;
    file.writeUInt32LE(table.rva, entry);
    file.writeUInt32LE(table.size, entry + 4);
  }
  if (imports.length) {
    const table = allocate((imports.length + 1) * 20, 4);
    table.size = (imports.length + 1) * 20;
    directory(1, table);
    imports.forEach((dll, index) => {
      const dllName = name(dll);
      const lookup = thunks();
      const iat = thunks();
      const entry = table.offset + index * 20;
      file.writeUInt32LE(lookup.rva, entry);
      file.writeUInt32LE(dllName.rva, entry + 12);
      file.writeUInt32LE(iat.rva, entry + 16);
    });
  }
  if (delayImports.length) {
    const table = allocate((delayImports.length + 1) * 32, 4);
    table.size = (delayImports.length + 1) * 32;
    directory(13, table);
    delayImports.forEach((dll, index) => {
      const dllName = name(dll);
      const handle = allocate(8, 8);
      const iat = thunks();
      const lookup = thunks();
      const entry = table.offset + index * 32;
      const pointer = value => delayUsesVa ? Number(imageBase) + value : value;
      file.writeUInt32LE(delayUsesVa ? 0 : 1, entry);
      file.writeUInt32LE(pointer(dllName.rva), entry + 4);
      file.writeUInt32LE(pointer(handle.rva), entry + 8);
      file.writeUInt32LE(pointer(iat.rva), entry + 12);
      file.writeUInt32LE(pointer(lookup.rva), entry + 16);
    });
  }
  return file;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    const values = {};
    for (let index = 0; index < args.length; index += 2) {
      if (!['--out', '--imports', '--delay-imports'].includes(args[index]) || index + 1 >= args.length || values[args[index]] !== undefined) {
        throw new Error('Use --out path [--imports comma-separated.dll] [--delay-imports comma-separated.dll].');
      }
      values[args[index]] = args[index + 1];
    }
    if (!values['--out']) throw new Error('Fixture output path is required.');
    const parse = value => value === '' ? [] : value.split(',');
    const options = { ...(values['--imports'] !== undefined ? { imports: parse(values['--imports']) } : {}),
      ...(values['--delay-imports'] !== undefined ? { delayImports: parse(values['--delay-imports']) } : {}) };
    writeFileSync(resolve(values['--out']), createPeFixture(options), { flag: 'wx' });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
