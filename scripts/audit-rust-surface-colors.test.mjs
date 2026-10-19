import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  checkIncludeContracts,
  collectRustColorLiterals,
  createRustSurfaceColorReport,
  findRustTestRegions,
  maskRustComments,
  normalizeRustHexColor,
  ownerMatchesFinding,
} from './audit-rust-surface-colors.mjs';

const repositoryRoot = process.cwd();

function temporaryRoot() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'rust-surface-audit-'));
}

function writeSource(root, relativePath, content) {
  const target = path.join(root, relativePath);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, content, 'utf8');
  return target;
}

function baselineFor(budgets) {
  const root = temporaryRoot();
  const baselinePath = path.join(root, 'baseline.json');
  fs.writeFileSync(baselinePath, JSON.stringify({ version: 1, budgets }), 'utf8');
  return baselinePath;
}

function fixtureSurface({ owners = [], contracts = [], baseline } = {}) {
  return {
    id: 'fixture',
    root: 'shell',
    audit: {
      engine: 'rust-surface',
      ...(baseline ? { baseline } : {}),
      ...(contracts.length > 0 ? { includeContracts: contracts } : {}),
      ...(owners.length > 0 ? { rawColorOwners: owners } : {}),
    },
  };
}

test('normalizeRustHexColor expands short hex and keeps alpha', () => {
  assert.equal(normalizeRustHexColor('#FFF'), '#ffffff');
  assert.equal(normalizeRustHexColor('AABBCC'), '#aabbcc');
  assert.equal(normalizeRustHexColor('#ffffff2e'), '#ffffff2e');
  assert.equal(normalizeRustHexColor('#ffff'), '#ffffffff');
  assert.equal(normalizeRustHexColor('#12345'), null);
});

test('maskRustComments blanks comments without moving offsets', () => {
  const source = 'let a = 1; // #ff0000\nlet b = 2; /* #00ff00 */\nlet c = 3;\n';
  const masked = maskRustComments(source);
  assert.equal(masked.length, source.length);
  assert.equal(masked.split('\n').length, source.split('\n').length);
  assert.ok(!masked.includes('#ff0000'));
  assert.ok(!masked.includes('#00ff00'));
  assert.ok(masked.includes('let c = 3;'));
});

test('findRustTestRegions covers modules, functions, and cfg(test) use statements', () => {
  const source = [
    'fn production() {}',
    '#[cfg(test)]',
    'mod tests {',
    '    fn nested() { if true { let x = 1; } }',
    '}',
    '#[cfg(test)]',
    'use super::*;',
    'fn after() {}',
  ].join('\n');
  const regions = findRustTestRegions(source);
  assert.equal(regions.length, 2);
  const covered = (needle) => regions.some(([start, end]) => {
    const index = source.indexOf(needle);
    return index >= start && index < end;
  });
  assert.ok(covered('fn nested'));
  assert.ok(covered('use super::*;'));
  assert.ok(!covered('fn production'));
  assert.ok(!covered('fn after'));
});

test('collectRustColorLiterals reads string and integer hex forms only', () => {
  const source = [
    'const A: u32 = 0x60a5fa;',
    'const B: &str = "#ffffff";',
    'const C: &str = "#12";',
    'const D: &str = "not-a-color";',
    '// const E: &str = "#abcdef";',
  ].join('\n');
  const { findings } = collectRustColorLiterals('src/lib.rs', source);
  assert.deepEqual(findings.map(finding => [finding.color, finding.form, finding.line]), [
    ['#60a5fa', 'integer-hex', 1],
    ['#ffffff', 'string-hex', 2],
  ]);
});

test('ownerMatchesFinding honors path, marker region, and line pattern', () => {
  const finding = { file: 'src/terminal/colors.rs', index: 40, line: 3, lineText: '    0x000000, 0xffffff,' };
  const region = [10, 80];
  assert.equal(ownerMatchesFinding({ kind: 'protocol-palette', file: 'src/terminal/colors.rs' }, finding, null), true);
  assert.equal(ownerMatchesFinding({ kind: 'protocol-palette', file: 'src/other.rs' }, finding, region), false);
  assert.equal(
    ownerMatchesFinding({ kind: 'protocol-palette', file: 'src/terminal/colors.rs', startMarker: 'a', endMarker: 'b' }, finding, null),
    false
  );
  assert.equal(
    ownerMatchesFinding({ kind: 'protocol-palette', file: 'src/terminal/colors.rs', startMarker: 'a', endMarker: 'b' }, { ...finding, index: 90 }, region),
    false
  );
  assert.equal(
    ownerMatchesFinding({ kind: 'protocol-palette', pathPrefixes: ['src/terminal'], linePattern: '0x000000' }, finding, region),
    true
  );
  assert.equal(
    ownerMatchesFinding({ kind: 'protocol-palette', pathPrefixes: ['src/terminal'], linePattern: '^\\s*0xdeadbeef' }, finding, region),
    false
  );
});

test('checkIncludeContracts reports missing embedded sources', () => {
  const root = temporaryRoot();
  writeSource(root, 'shell/src/theme.rs', 'const T: &str = include_str!("../../tokens/dark.tokens.json");\n');
  writeSource(root, 'tokens/dark.tokens.json', '{}\n');
  const surface = {
    id: 'fixture',
    root: 'shell',
    audit: {
      engine: 'rust-surface',
      includeContracts: ['tokens/dark.tokens.json', 'tokens/light.tokens.json'],
    },
  };
  const result = checkIncludeContracts(surface, root);
  assert.deepEqual(result.satisfied, ['tokens/dark.tokens.json']);
  assert.deepEqual(result.missing, ['tokens/light.tokens.json']);
});

test('createRustSurfaceColorReport fails on unowned literals and passes declared owners', () => {
  const root = temporaryRoot();
  writeSource(root, 'shell/src/palette.rs', [
    'pub const BASE: [u32; 2] = [0x000000, 0xffffff];',
    'fn chrome() { let accent = "#60a5fa"; }',
  ].join('\n'));
  const surface = fixtureSurface({
    owners: [
      {
        kind: 'protocol-palette',
        file: 'src/palette.rs',
        startMarker: 'pub const BASE',
        endMarker: '];',
        reason: 'fixture protocol palette owner for the audit test',
      },
    ],
  });

  const report = createRustSurfaceColorReport(surface, { repositoryRoot: root });
  assert.equal(report.productionColorOccurrences, 3);
  assert.equal(report.ownedColorOccurrences, 2);
  assert.equal(report.unownedColorOccurrences, 1);
  assert.equal(report.unownedColors[0].color, '#60a5fa');
  assert.equal(report.failures.length, 1);
  assert.match(report.failures[0], /src\/palette\.rs:2 raw string-hex color #60a5fa/);
});

test('createRustSurfaceColorReport flags stale owners and missing include contracts', () => {
  const root = temporaryRoot();
  writeSource(root, 'shell/src/chrome.rs', 'fn chrome() { let accent = "#60a5fa"; }\n');
  const surface = fixtureSurface({
    owners: [
      {
        kind: 'protocol-palette',
        file: 'src/chrome.rs',
        startMarker: 'does not exist',
        endMarker: '];',
        reason: 'fixture stale owner to prove the registry ratchet fails',
      },
    ],
    contracts: ['tokens/dark.tokens.json'],
  });

  const report = createRustSurfaceColorReport(surface, { repositoryRoot: root });
  assert.equal(report.staleOwners, 1);
  assert.equal(report.includeContractsMissing, 1);
  assert.ok(report.failures.some(failure => failure.includes('is stale')));
  assert.ok(report.failures.some(failure => failure.includes('no longer embeds declared color source')));
});

test('createRustSurfaceColorReport counts test fixtures separately and enforces its baseline', () => {
  const root = temporaryRoot();
  writeSource(root, 'shell/src/theme.rs', [
    'pub const FALLBACK: &str = "#000000";',
    '#[cfg(test)]',
    'mod tests {',
    '    #[test]',
    '    fn fixture() { assert_eq!(color("#101010"), 16); }',
    '}',
  ].join('\n'));
  const baselinePath = baselineFor({ productionColorOccurrences: { max: 2 } });
  const surface = fixtureSurface({ baseline: path.relative(root, baselinePath) });

  const report = createRustSurfaceColorReport(surface, { repositoryRoot: root });
  assert.equal(report.productionColorOccurrences, 1);
  assert.equal(report.testColorOccurrences, 1);
  assert.ok(report.failures.some(failure => failure.includes('below baseline')));
});

test('the shipped gpui-shell surface stays within its declared color budgets', () => {
  const registry = JSON.parse(fs.readFileSync(path.join(repositoryRoot, 'scripts/frontend-color-surface-registry.json'), 'utf8'));
  const surface = registry.surfaces.find(candidate => candidate.id === 'gpui-shell');
  assert.ok(surface, 'gpui-shell surface must be registered');
  const report = createRustSurfaceColorReport(surface, { repositoryRoot });
  assert.ok(report.filesScanned > 0);
  assert.equal(report.unownedColorOccurrences, 0);
  assert.equal(report.includeContractsMissing, 0);
  assert.equal(report.staleOwners, 0);
  assert.deepEqual(report.failures, []);
});
