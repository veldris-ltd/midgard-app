#!/usr/bin/env node
// Generate the Rust console read models from the TypeScript contract.
// [FR-C-1] [AC-C-13]
//
//   node clients/demo/tools/gen-read-models.mjs --write [--ffi-out clients/ffi/src/generated]
//   node clients/demo/tools/gen-read-models.mjs --check [--ffi-out clients/ffi/src/generated]
//   (--src-root <dir> reads the TypeScript from <dir> instead of the repository; tests)
//
// Reads sdk/ts/src/types.ts and sdk/ts/src/products/{admin,bifrost,forge,heimdall,mimir}.ts
// as text and understands only what those files use: `export type X = …;` aliases and
// `export interface X { name?: Type; }` blocks. Anything else is an error, so a new
// shape in the contract fails here instead of decoding wrongly: every `interface`,
// `enum` and `type X =`/`type X<` declaration it did not consume (an `extends` or
// generic interface, an unexported one, an object or generic alias) stops it, and so
// does a type name declared twice with different text. Classes and functions are
// behaviour, not contract shapes, and are ignored. Tested by
// tools/ci/tests/gen-read-models.test.sh.
//
// Output is formatted with rustfmt, so `cargo fmt --all --check` agrees with --check.
// It must be the pinned toolchain's rustfmt (rust-toolchain.toml): put the rustup
// proxies first on PATH (`export PATH=/opt/homebrew/opt/rustup/bin:$PATH` on this Mac).
// A different rustfmt, such as Homebrew's, formats differently and --check then
// reports every file as stale.
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '../../..');
const CORE_OUT = join(ROOT, 'clients/core/src/consoles');
const srcRootIdx = process.argv.indexOf('--src-root');
const SRC_ROOT = srcRootIdx > 0 ? resolve(process.argv[srcRootIdx + 1]) : ROOT;
const SOURCES = [
  { module: 'midgard', file: 'sdk/ts/src/types.ts', prefix: 'Midgard',
    shared: ['AuditRecord', 'PolicyContext'],
    skip: ['QueryRequest', 'QueryCondition', 'ApplyActionRequest'] },
  { module: 'admin', file: 'sdk/ts/src/products/admin.ts' },
  { module: 'bifrost', file: 'sdk/ts/src/products/bifrost.ts' },
  { module: 'forge', file: 'sdk/ts/src/products/forge.ts' },
  { module: 'heimdall', file: 'sdk/ts/src/products/heimdall.ts' },
  { module: 'mimir', file: 'sdk/ts/src/products/mimir.ts' },
];
const RUST_KEYWORDS = new Set(['type', 'ref', 'match', 'move', 'loop', 'impl', 'fn', 'mod', 'use',
  'where', 'self', 'super', 'crate', 'in', 'as', 'const', 'static', 'struct', 'enum', 'trait',
  'async', 'await', 'dyn', 'box', 'yield', 'try', 'override', 'final', 'macro', 'priv', 'virtual']);

const die = (msg) => { console.error(`gen-read-models: ${msg}`); process.exit(1); };

// --- lexing helpers -----------------------------------------------------------------
function stripComments(src) {
  let out = '', i = 0, q = null;
  while (i < src.length) {
    const c = src[i], n = src[i + 1];
    if (q) { out += c; if (c === '\\') { out += n; i += 2; continue; } if (c === q) q = null; i++; continue; }
    if (c === "'" || c === '"' || c === '`') { q = c; out += c; i++; continue; }
    if (c === '/' && n === '*') { const e = src.indexOf('*/', i + 2); i = e < 0 ? src.length : e + 2; continue; }
    if (c === '/' && n === '/') { const e = src.indexOf('\n', i); i = e < 0 ? src.length : e; continue; }
    out += c; i++;
  }
  return out;
}
// The same text with the inside of every string literal blanked (same length, so
// indices still line up): a declaration keyword inside a string is not a declaration.
function blankStrings(src) {
  let out = '', q = null;
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (q) {
      if (c === '\\') { out += '  '; i++; continue; }
      if (c === q) { q = null; out += c; continue; }
      out += c === '\n' ? c : ' ';
      continue;
    }
    if (c === "'" || c === '"' || c === '`') q = c;
    out += c;
  }
  return out;
}
// Split `s` on `sep` at bracket depth 0, outside string literals.
function splitTop(s, sep) {
  const parts = []; let depth = 0, q = null, cur = '';
  for (const c of s) {
    if (q) { cur += c; if (c === q) q = null; continue; }
    if (c === "'" || c === '"') { q = c; cur += c; continue; }
    if ('{[(<'.includes(c)) depth++;
    if ('}])>'.includes(c)) depth--;
    if (c === sep && depth === 0) { parts.push(cur); cur = ''; continue; }
    cur += c;
  }
  parts.push(cur);
  return parts.map((p) => p.trim()).filter((p) => p !== '');
}
function blockAfter(src, openIdx) { // src[openIdx] === '{' → index of the matching '}'
  let depth = 0;
  for (let i = openIdx; i < src.length; i++) {
    if (src[i] === '{') depth++;
    if (src[i] === '}' && --depth === 0) return i;
  }
  die('unbalanced braces');
}

// --- parsing ------------------------------------------------------------------------
const aliases = new Map();     // name -> type text (all files)
const interfaces = [];         // { module, name, rustName, members:[{name, optional, type}] }
const rustNameOf = new Map();  // TS interface name -> Rust struct name

function parseMembers(body, where) {
  return splitTop(body.replace(/\n/g, ';'), ';').map((m) => {
    const mm = m.match(/^(readonly\s+)?([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([\s\S]+)$/);
    if (!mm) die(`${where}: cannot read member "${m}"`);
    if (/^\(.*\)\s*=>/.test(mm[4]) || mm[4].includes('Promise<')) die(`${where}: method member "${m}"`);
    return { name: mm[2], optional: !!mm[3], type: mm[4].trim() };
  });
}

// Every type name, once: a second declaration with different text would silently
// replace the first. An identical redeclaration is the same type and is kept once.
const declared = new Map(); // name -> "kind:normalised text"
function declare(name, kind, text, where) {
  const key = `${kind}:${text.replace(/\s+/g, ' ').trim()}`;
  if (!declared.has(name)) { declared.set(name, key); return true; }
  if (declared.get(name) !== key) die(`${where}: declared twice: ${name}, with different text`);
  return false;
}

for (const src of SOURCES) {
  const text = stripComments(readFileSync(join(SRC_ROOT, src.file), 'utf8'));
  const consumed = new Set(); // indices of the declaration keywords read below
  // `[^;{]+`: an object-type alias (`= { … }`) is not read here, so the scan below stops on it.
  for (const m of text.matchAll(/export\s+type\s+([A-Za-z0-9_]+)\s*=\s*([^;{]+);/g)) {
    consumed.add(m.index + m[0].indexOf('type'));
    if (declare(m[1], 'type', m[2], src.file)) aliases.set(m[1], m[2].trim());
  }
  for (const m of text.matchAll(/export\s+interface\s+([A-Za-z0-9_]+)\s*\{/g)) {
    const name = m[1];
    const open = m.index + m[0].length - 1;
    const body = text.slice(open + 1, blockAfter(text, open));
    consumed.add(m.index + m[0].indexOf('interface'));
    if (!declare(name, 'interface', body, src.file)) continue;
    if (name.endsWith('Gateway') || (src.skip ?? []).includes(name)) continue;
    const isShared = (src.shared ?? []).includes(name);
    const module = isShared ? 'common' : src.module;
    const rustName = isShared || !src.prefix ? name : `${src.prefix}${name}`;
    rustNameOf.set(name, rustName);
    interfaces.push({ module, name, rustName, source: src.file, members: parseMembers(body, `${src.file} ${name}`) });
  }
  // Anything declared that the two loops above did not read is a shape this generator
  // does not understand: stop rather than leave it out of the Rust.
  const bare = blankStrings(text);
  const scans = [
    /\b(interface|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)/g,
    /\b(type)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*[=<]/g,
  ];
  for (const re of scans) {
    for (const m of bare.matchAll(re)) {
      if (!consumed.has(m.index)) die(`${src.file}: not consumed: ${m[1]} ${m[2]} (only \`export interface X {\` and \`export type X = …;\` are understood)`);
    }
  }
}

// Type text -> IR: {k:'str'|'f64'|'u32'|'bool'|'kv'|'pair'} | {k:'vec',of} | {k:'opt',of} | {k:'ref',name} | {k:'obj',members}
function typeOf(t, ctx) {
  t = t.trim();
  if (t.startsWith('(') && t.endsWith(')')) return typeOf(t.slice(1, -1), ctx);
  const union = splitTop(t, '|');
  if (union.length > 1) {
    const rest = union.filter((u) => u !== 'null' && u !== 'undefined');
    if (rest.length < union.length) return { k: 'opt', of: typeOf(rest.join(' | '), ctx) };
    // An alias of a literal union inside a union (`MimirHealth | 'gated'`) widens it.
    const flat = rest.flatMap((u) => (aliases.has(u) && !rustNameOf.has(u) ? splitTop(aliases.get(u), '|') : [u]));
    if (flat.every((u) => /^'[^']*'$/.test(u))) return { k: 'str', doc: flat.join(' | ') };
    if (flat.every((u) => /^\d+$/.test(u))) return { k: 'u32', doc: flat.join(' | ') };
    die(`${ctx}: unsupported union "${t}"`);
  }
  if (t.endsWith('[]')) return { k: 'vec', of: typeOf(t.slice(0, -2), ctx) };
  const arr = t.match(/^Array<([\s\S]+)>$/);
  if (arr) return { k: 'vec', of: typeOf(arr[1], ctx) };
  // A [label, value] tuple (ForgeTransform.contractChecks): a pair of strings, in order.
  if (/^\[\s*string\s*,\s*string\s*\]$/.test(t)) return { k: 'pair' };
  if (t === 'string') return { k: 'str' };
  if (t === 'number') return { k: 'f64' };
  if (t === 'boolean') return { k: 'bool' };
  if (/^Record<\s*string\s*,\s*string\s*>$/.test(t)) return { k: 'kv' };
  if (/^'[^']*'$/.test(t)) return { k: 'str', doc: t };
  if (t.startsWith('{') && t.endsWith('}')) return { k: 'obj', members: parseMembers(t.slice(1, -1), ctx) };
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(t)) {
    if (rustNameOf.has(t)) return { k: 'ref', name: rustNameOf.get(t) };
    if (aliases.has(t)) return typeOf(aliases.get(t), `${ctx} (alias ${t})`);
  }
  die(`${ctx}: unsupported type "${t}"`);
}

// --- emitting -----------------------------------------------------------------------
const snake = (s) => {
  const x = s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/([A-Z])([A-Z][a-z])/g, '$1_$2').toLowerCase();
  return RUST_KEYWORDS.has(x) ? (x === 'type' ? 'type_name' : `${x}_`) : x;
};
const pascal = (s) => s.charAt(0).toUpperCase() + s.slice(1);

function rustType(ir, emitNested, parent, field) {
  switch (ir.k) {
    case 'str': return 'String';
    case 'f64': return 'f64';
    case 'u32': return 'u32';
    case 'bool': return 'bool';
    case 'kv': return 'crate::consoles::decode::KeyValues';
    case 'pair': return '(String, String)';
    case 'vec': return `Vec<${rustType(ir.of, emitNested, parent, field)}>`;
    case 'opt': return `Option<${rustType(ir.of, emitNested, parent, field)}>`;
    case 'ref': return ir.name;
    case 'obj': { const n = `${parent}${pascal(field)}`; emitNested(n, ir.members); return n; }
  }
}
function docOf(ir) { return ir.doc ? ir.doc : ir.of ? docOf(ir.of) : null; }

function structFor(name, members, source, ctx, nested) {
  const fields = members.map((m) => {
    const base = typeOf(m.type, `${ctx}.${m.name}`);
    const ir = m.optional && base.k !== 'opt' ? { k: 'opt', of: base } : base;
    const ty = rustType(ir, (n, ms) => nested.push(structFor(n, ms, source, `${ctx}.${m.name}`, nested)), name, m.name);
    return { ts: m.name, rs: snake(m.name), ty, opt: ir.k === 'opt', doc: docOf(ir) };
  });
  const decl = fields.map((f) =>
    `${f.doc ? `    /// One of: \`${f.doc}\`.\n` : `    /// \`${f.ts}\`.\n`}    pub ${f.rs}: ${f.ty},`).join('\n');
  const known = fields.map((f) => JSON.stringify(f.ts)).join(', ');
  // The allowed values of every literal-union field, so callers can check a table
  // (for example the marking levels) covers them.
  const consts = fields.filter((f) => f.doc).map((f) => {
    const strs = [...f.doc.matchAll(/'([^']*)'/g)].map((x) => x[1]);
    const nums = f.doc.split('|').map((x) => x.trim()).filter((x) => /^\d+$/.test(x));
    const [ty, vals] = strs.length ? ['&str', strs.map((x) => JSON.stringify(x))] : ['u32', nums];
    const id = `${snake(name).toUpperCase()}_${f.rs.toUpperCase()}_VALUES`;
    return `/// The values \`${name}.${f.rs}\` may take, in contract order.\npub const ${id}: &[${ty}] = &[${[...new Set(vals)].join(', ')}];\n`;
  }).join('\n');
  const init = fields.map((f) =>
    `            ${f.rs}: ${f.opt ? 'opt_field' : 'field'}(o, ${JSON.stringify(f.ts)}, path)?,`).join('\n');
  return { name, fields, rust:
`/// \`${name}\`, from \`${source}\`.
#[derive(Debug, Clone, PartialEq)]
pub struct ${name} {
${decl}
}

impl FromJson for ${name} {
    fn from_json(v: &Json, path: &str) -> Result<Self, DecodeError> {
        let o = object(v, path, &[${known}])?;
        Ok(Self {
${init}
        })
    }
}
${consts ? `\n${consts}` : ''}` };
}

function rustfmt(src) {
  return execFileSync('rustfmt', ['--edition', '2021', '--config-path', join(ROOT, 'rustfmt.toml')],
    { input: src, cwd: ROOT, encoding: 'utf8' });
}

const modules = ['common', ...SOURCES.map((s) => s.module)];
const outputs = new Map(); // path -> content
const ffiOutIdx = process.argv.indexOf('--ffi-out');
const FFI_OUT = ffiOutIdx > 0 ? join(ROOT, process.argv[ffiOutIdx + 1]) : null;
const ffiParts = new Map(modules.map((m) => [m, []]));

for (const mod of modules) {
  const defs = [];
  for (const itf of interfaces.filter((i) => i.module === mod)) {
    const nested = [];
    const s = structFor(itf.rustName, itf.members, itf.source, `${itf.source} ${itf.name}`, nested);
    for (const d of [s, ...nested]) { defs.push(d.rust); ffiParts.get(mod).push(d); }
  }
  const sources = [...new Set(interfaces.filter((i) => i.module === mod).map((i) => i.source))].join(', ');
  const head =
`//! @generated by clients/demo/tools/gen-read-models.mjs from ${sources || 'nothing'}.
//! Do not edit: run \`node clients/demo/tools/gen-read-models.mjs --write\`. [FR-C-1] [AC-C-13]
#![allow(clippy::all)]

#[allow(unused_imports)]
use crate::consoles::decode::{field, object, opt_field, DecodeError, FromJson};
#[allow(unused_imports)]
use attest::json::Json;
${mod === 'common' ? '' : '#[allow(unused_imports)]\nuse crate::consoles::common::*;\n'}`;
  outputs.set(join(CORE_OUT, `${mod}.rs`), rustfmt(`${head}\n${defs.join('\n')}`));
}

if (FFI_OUT) {
  // FFI mirrors: the same fields with uniffi derives, and From<core> conversions.
  const conv = (ty, expr) => {
    if (ty === 'crate::consoles::decode::KeyValues') return `${expr}.0.into_iter().map(|(key, value)| KeyValue { key, value }).collect()`;
    if (ty === '(String, String)') return `{ let (key, value) = ${expr}; KeyValue { key, value } }`;
    const v = ty.match(/^Vec<(.+)>$/); if (v) return `${expr}.into_iter().map(|x| ${conv(v[1], 'x')}).collect()`;
    const o = ty.match(/^Option<(.+)>$/); if (o) return `${expr}.map(|x| ${conv(o[1], 'x')})`;
    return ['String', 'f64', 'u32', 'bool'].includes(ty) ? expr : `${expr}.into()`;
  };
  const ffiTy = (ty) => ty.replace(/crate::consoles::decode::KeyValues/g, 'Vec<KeyValue>').replace(/\(String, String\)/g, 'KeyValue');
  for (const mod of modules) {
    const body = ffiParts.get(mod).map((d) =>
`#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ${d.name} {
${d.fields.map((f) => `    pub ${f.rs}: ${ffiTy(f.ty)},`).join('\n')}
}

impl From<core::${d.name}> for ${d.name} {
    fn from(x: core::${d.name}) -> Self {
        Self {
${d.fields.map((f) => `            ${f.rs}: ${conv(f.ty, `x.${f.rs}`)},`).join('\n')}
        }
    }
}
`).join('\n');
    const head =
`//! @generated by clients/demo/tools/gen-read-models.mjs: UniFFI mirrors of
//! midgard_client_core::consoles::${mod}. Do not edit. [FR-C-1] [DD-6]
#![allow(clippy::all)]

#[allow(unused_imports)]
use super::common::*;
#[allow(unused_imports)]
use super::KeyValue;
use midgard_client_core::consoles::${mod} as core;
`;
    outputs.set(join(FFI_OUT, `${mod}.rs`), rustfmt(`${head}\n${body}`));
  }
}

const mode = process.argv.includes('--write') ? 'write' : process.argv.includes('--check') ? 'check' : die('use --write or --check');
let stale = 0;
for (const [path, content] of outputs) {
  if (mode === 'write') { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, content); continue; }
  if (!existsSync(path) || readFileSync(path, 'utf8') !== content) { console.error(`stale: ${path.slice(ROOT.length + 1)}`); stale++; }
}
if (stale) die(`${stale} generated file(s) differ from the TypeScript contract; run with --write`);
console.log(`gen-read-models: ${interfaces.length} interfaces, ${outputs.size} file(s) ${mode === 'write' ? 'written' : 'up to date'}`);
