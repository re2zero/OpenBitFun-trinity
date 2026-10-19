// Color-surface audit engine for Rust-rendered native surfaces.
//
// The GPUI shell paints with code rather than CSS: design colors must come from
// the canonical OpenBitFun authoring tokens (embedded through `include_str!`)
// instead of literals scattered across the renderer. This engine scans
// production Rust sources and classifies every raw color literal as either
// covered by a registry-declared narrow owner (for example the xterm protocol
// palette) or unowned debt that the surface baseline ratchets down.
//
// Test modules, `examples/`, and `tests/` are not shipped UI, so their fixtures
// are counted separately and never budgeted here.

import fs from 'node:fs';
import path from 'node:path';

import { checkBudgetBaseline, cliHexColorDistance } from './theme-color-audit-utils.mjs';

export { checkBudgetBaseline as checkBaseline } from './theme-color-audit-utils.mjs';

const HEX_COLOR_LENGTHS = new Set([3, 4, 6, 8]);
const STRING_HEX_PATTERN = /"#([0-9a-fA-F]{3,8})"/g;
const INTEGER_HEX_PATTERN = /\b0x([0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b/g;
const CFG_TEST_PATTERN = /#\[cfg\(test\)\]/g;
const INCLUDE_STR_PATTERN = /include_str!\(\s*"([^"]+)"\s*\)/g;

export const DEFAULT_NEAR_THRESHOLD = 10;

function normalizePath(value) {
  return value.split(path.sep).join('/');
}

function normalizeRelativePath(value) {
  return normalizePath(path.normalize(value)).replace(/^\.\//, '').replace(/\/$/, '');
}

function splitLines(content) {
  return content.split('\n');
}

function lineAt(lines, index, content) {
  return lines[content.slice(0, index).split('\n').length - 1] ?? '';
}

/** Expands three-digit hex and lowercases; eight-digit values keep their alpha. */
export function normalizeRustHexColor(value) {
  const raw = value.replace(/^#/, '').toLowerCase();
  if (!HEX_COLOR_LENGTHS.has(raw.length)) return null;
  if (raw.length === 3 || raw.length === 4) {
    return `#${raw.split('').map(character => character.repeat(2)).join('')}`;
  }
  return `#${raw}`;
}

function maskRange(characters, start, end) {
  for (let index = start; index < end; index += 1) {
    if (characters[index] !== '\n') characters[index] = ' ';
  }
}

/**
 * Blanks `//` and block comments while preserving offsets and line numbers, so
 * a color literal mentioned in prose never counts as a renderer color.
 */
export function maskRustComments(sourceText) {
  const characters = Array.from(sourceText);
  let index = 0;
  while (index < sourceText.length) {
    if (sourceText.startsWith('//', index)) {
      const end = sourceText.indexOf('\n', index);
      const stop = end < 0 ? sourceText.length : end;
      maskRange(characters, index, stop);
      index = stop;
      continue;
    }
    if (sourceText.startsWith('/*', index)) {
      const end = sourceText.indexOf('*/', index + 2);
      const stop = end < 0 ? sourceText.length : end + 2;
      maskRange(characters, index, stop);
      index = stop;
      continue;
    }
    index += 1;
  }
  return characters.join('');
}

function braceEnd(sourceText, openIndex) {
  let depth = 0;
  for (let index = openIndex; index < sourceText.length; index += 1) {
    const character = sourceText[index];
    if (character === '{') depth += 1;
    else if (character === '}') {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return sourceText.length;
}

/**
 * Test-only regions as `[start, end)` offsets. Each `#[cfg(test)]` attribute
 * owns the following item: a braced item runs to its matching brace, a
 * statement (for example `use ...;`) runs to its semicolon.
 */
export function findRustTestRegions(sourceText) {
  const regions = [];
  CFG_TEST_PATTERN.lastIndex = 0;
  let match;
  while ((match = CFG_TEST_PATTERN.exec(sourceText)) !== null) {
    const start = match.index;
    let cursor = start + match[0].length;
    while (cursor < sourceText.length && /\s/.test(sourceText[cursor])) cursor += 1;
    while (sourceText.startsWith('#[', cursor)) {
      const attributeEnd = sourceText.indexOf(']', cursor);
      if (attributeEnd < 0) break;
      cursor = attributeEnd + 1;
      while (cursor < sourceText.length && /\s/.test(sourceText[cursor])) cursor += 1;
    }
    const braceIndex = sourceText.indexOf('{', cursor);
    const semicolonIndex = sourceText.indexOf(';', cursor);
    if (semicolonIndex >= 0 && (braceIndex < 0 || semicolonIndex < braceIndex)) {
      regions.push([start, semicolonIndex + 1]);
      continue;
    }
    regions.push([start, braceIndex < 0 ? sourceText.length : braceEnd(sourceText, braceIndex)]);
  }
  return regions;
}

function isInsideRegions(regions, index) {
  return regions.some(([start, end]) => index >= start && index < end);
}

/**
 * Raw color literals in one Rust source. `findings` covers the whole file (the
 * caller classifies production vs. test by offset); string-hex and integer-hex
 * forms are the two literal shapes a Rust renderer can hard-code.
 */
export function collectRustColorLiterals(relativePath, sourceText) {
  const masked = maskRustComments(sourceText);
  const lines = splitLines(masked);
  const findings = [];
  const seen = new Set();

  const record = (index, value, form) => {
    const color = normalizeRustHexColor(value);
    if (!color) return;
    const key = `${relativePath}:${index}:${color}`;
    if (seen.has(key)) return;
    seen.add(key);
    findings.push({
      file: relativePath,
      index,
      line: masked.slice(0, index).split('\n').length,
      lineText: lineAt(lines, index, masked),
      color,
      form,
    });
  };

  for (const match of masked.matchAll(STRING_HEX_PATTERN)) {
    record(match.index + match[0].indexOf('#'), match[1], 'string-hex');
  }
  for (const match of masked.matchAll(INTEGER_HEX_PATTERN)) {
    record(match.index + match[0].indexOf('0x'), match[1], 'integer-hex');
  }

  return {
    findings: findings.sort((left, right) => left.index - right.index),
    testRegions: findRustTestRegions(masked),
  };
}

function ownerPaths(owner) {
  return [owner.file, ...(owner.files ?? []), ...(owner.pathPrefixes ?? [])]
    .filter(Boolean)
    .map(normalizeRelativePath);
}

/**
 * `region` is the owner's declared `startMarker`/`endMarker` span in the
 * finding's own file (`null` when the file does not contain it).
 */
export function ownerMatchesFinding(owner, finding, region) {
  const pathMatches = ownerPaths(owner).some(ownerPath => (
    finding.file === ownerPath || finding.file.startsWith(`${ownerPath}/`)
  ));
  if (!pathMatches) return false;
  if (owner.startMarker && owner.endMarker) {
    if (!region) return false;
    if (finding.index < region[0] || finding.index >= region[1]) return false;
  }
  if (owner.linePattern && !new RegExp(owner.linePattern).test(finding.lineText)) return false;
  return true;
}

function ownerRegionIn(owner, maskedSource) {
  if (!owner.startMarker || !owner.endMarker) return null;
  const start = maskedSource.indexOf(owner.startMarker);
  if (start < 0) return null;
  const end = maskedSource.indexOf(owner.endMarker, start + owner.startMarker.length);
  if (end < 0) return null;
  return [start, end + owner.endMarker.length];
}

function findNearPairs(findings, threshold) {
  const colors = Array.from(new Set(findings.map(finding => finding.color))).sort();
  const pairs = [];
  for (let i = 0; i < colors.length; i += 1) {
    for (let j = i + 1; j < colors.length; j += 1) {
      const distance = cliHexColorDistance(colors[i], colors[j]);
      if (distance > 0 && distance <= threshold) {
        pairs.push({ a: colors[i], b: colors[j], distance: Number(distance.toFixed(2)) });
      }
    }
  }
  pairs.sort((left, right) => left.distance - right.distance || left.a.localeCompare(right.a));
  return pairs;
}

function walkRustFiles(root) {
  if (!fs.existsSync(root)) return [];
  const files = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === 'target' || entry.name === 'node_modules') continue;
        stack.push(fullPath);
      } else if (entry.isFile() && path.extname(entry.name).toLowerCase() === '.rs') {
        files.push(fullPath);
      }
    }
  }
  return files.sort((left, right) => left.localeCompare(right));
}

/**
 * Verifies the surface embeds its declared color sources with `include_str!`.
 * A design surface that stops reading the canonical authoring tokens has left
 * governance even when no literal was added, so this is a failure, not a note.
 */
export function checkIncludeContracts(surface, repositoryRoot) {
  const contracts = (surface.audit.includeContracts ?? []).map(normalizeRelativePath);
  const includedPaths = new Set();
  for (const file of walkRustFiles(path.join(repositoryRoot, surface.root, 'src'))) {
    const content = fs.readFileSync(file, 'utf8');
    for (const match of content.matchAll(INCLUDE_STR_PATTERN)) {
      includedPaths.add(normalizePath(path.relative(repositoryRoot, path.resolve(path.dirname(file), match[1]))));
    }
  }
  return {
    contracts,
    satisfied: contracts.filter(contract => includedPaths.has(contract)),
    missing: contracts.filter(contract => !includedPaths.has(contract)),
  };
}

export function createRustSurfaceColorReport(surface, { repositoryRoot = process.cwd() } = {}) {
  const root = path.join(repositoryRoot, surface.root);
  const files = walkRustFiles(path.join(root, 'src'));
  const owners = surface.audit.rawColorOwners ?? [];
  const failures = [];
  const ownerMatches = owners.map(owner => ({ kind: owner.kind, occurrences: 0 }));
  const productionFindings = [];
  const ownedFindings = [];
  const unownedFindings = [];
  let testColorOccurrences = 0;

  for (const file of files) {
    const relativePath = normalizePath(path.relative(root, file));
    const sourceText = fs.readFileSync(file, 'utf8');
    const masked = maskRustComments(sourceText);
    const { findings, testRegions } = collectRustColorLiterals(relativePath, sourceText);
    const regions = owners.map(owner => ownerRegionIn(owner, masked));

    for (const finding of findings) {
      if (isInsideRegions(testRegions, finding.index)) {
        testColorOccurrences += 1;
        continue;
      }
      productionFindings.push(finding);
      const ownerIndex = owners.findIndex((owner, index) => ownerMatchesFinding(owner, finding, regions[index]));
      if (ownerIndex < 0) {
        unownedFindings.push(finding);
        failures.push(
          `${surface.id}:${finding.file}:${finding.line} raw ${finding.form} color ${finding.color} ` +
            'is not covered by a declared narrow owner.'
        );
      } else {
        ownerMatches[ownerIndex].occurrences += 1;
        ownedFindings.push(finding);
      }
    }
  }

  for (const [index, entry] of ownerMatches.entries()) {
    if (entry.occurrences === 0) {
      const owner = owners[index];
      const declared = owner.file ?? (owner.files ?? []).join(', ') ?? (owner.pathPrefixes ?? []).join(', ');
      failures.push(
        `${surface.id} narrow owner ${owner.kind} (${declared}) is stale; remove or narrow the registry entry.`
      );
    }
  }

  const includes = checkIncludeContracts(surface, repositoryRoot);
  for (const missing of includes.missing) {
    failures.push(`${surface.id} no longer embeds declared color source ${missing} via include_str!.`);
  }

  const threshold = surface.audit.nearThreshold ?? DEFAULT_NEAR_THRESHOLD;
  const near = findNearPairs(unownedFindings, threshold);

  const report = {
    surfaceId: surface.id,
    engine: 'rust-surface',
    root: surface.root,
    filesScanned: files.length,
    owners: ownerMatches,
    ownerOccurrences: ownerMatches.reduce((total, entry) => total + entry.occurrences, 0),
    includeContracts: includes.contracts,
    includeContractsSatisfied: includes.satisfied,
    includeContractsMissing: includes.missing.length,
    missingIncludeContracts: includes.missing,
    staleOwners: ownerMatches.filter(entry => entry.occurrences === 0).length,
    productionColorOccurrences: productionFindings.length,
    productionUniqueColors: new Set(productionFindings.map(finding => finding.color)).size,
    ownedColorOccurrences: ownedFindings.length,
    ownedUniqueColors: new Set(ownedFindings.map(finding => finding.color)).size,
    unownedColorOccurrences: unownedFindings.length,
    unownedUniqueColors: new Set(unownedFindings.map(finding => finding.color)).size,
    unownedColors: unownedFindings.map(finding => ({
      file: finding.file,
      line: finding.line,
      color: finding.color,
      form: finding.form,
    })),
    testColorOccurrences,
    nearThreshold: threshold,
    unownedNearPairs: { nearTotal: near.length, near },
    failures: [],
  };

  if (surface.audit.baseline) {
    failures.push(...checkBudgetBaseline(report, path.join(repositoryRoot, surface.audit.baseline)));
  }
  report.failures = failures;
  return report;
}
