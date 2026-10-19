import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';

import { ProductDefinitionError, resolveProductDefinition } from './resolver.mjs';

const ROOT = resolve(import.meta.dirname, '..', '..');
const ACME = join(ROOT, 'products', 'fixtures', 'acme', 'product.jsonc');

test('default and custom members resolve through one deterministic contract', () => {
  const openbitfun = resolveProductDefinition({ rootDir: ROOT, member: 'desktop' });
  const desktop = resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'desktop' });
  const cli = resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'cli' });

  assert.equal(openbitfun.assembly.productId, 'openbitfun');
  assert.equal(openbitfun.assembly.dataNamespace, 'openbitfun');
  assert.equal(openbitfun.assembly.binaryName, 'openbitfun-desktop');
  assert.equal(openbitfun.assembly.bundleId, 'com.openbitfun.desktop');
  assert.equal(desktop.assembly.bundleId, 'com.acme.desktop');
  assert.equal(cli.assembly.binaryName, 'acme');
  assert.equal(cli.assembly.bundleId, undefined);
  assert.notEqual(desktop.assembly.assemblyDigest, cli.assembly.assemblyDigest);
  assert.equal(
    resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'desktop' })
      .assembly.assemblyDigest,
    desktop.assembly.assemblyDigest,
  );
});

test('localized names are validated against the shared locale contract', () => {
  const resolution = resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'desktop' });
  assert.equal(resolution.productNames['en-US'], 'Acme Desktop');
  assert.equal(resolution.productNames['zh-CN'], 'Acme 桌面版');
  assert.match(resolution.assembly.localeDigest, /^[a-f0-9]{64}$/);
});

test('schema version one rejects future owner sections instead of pretending to support them', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-product-c0a-'));
  const source = readFileSync(ACME, 'utf8').replace(
    /\n}\s*$/,
    ',\n  "assets": { "desktopAppIcon": "icon.png" }\n}\n',
  );
  const config = join(directory, 'product.jsonc');
  writeFileSync(config, source, 'utf8');

  assert.throws(
    () => resolveProductDefinition({ rootDir: ROOT, productConfig: config, member: 'desktop' }),
    (error) => error instanceof ProductDefinitionError && error.code === 'unknown_field',
  );
});

test('locale paths cannot escape the product definition directory', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-product-c0a-'));
  const source = readFileSync(ACME, 'utf8').replace('"./locales"', '"../"');
  const config = join(directory, 'product.jsonc');
  writeFileSync(config, source, 'utf8');

  assert.throws(
    () => resolveProductDefinition({ rootDir: ROOT, productConfig: config, member: 'desktop' }),
    (error) => error instanceof ProductDefinitionError && error.code === 'resource_path_escape',
  );
});

test('invalid member and unsafe binary identity fail with stable codes', () => {
  assert.throws(
    () => resolveProductDefinition({ rootDir: ROOT, member: 'server' }),
    (error) => error instanceof ProductDefinitionError && error.code === 'invalid_member',
  );

  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-product-c0a-'));
  cpSync(join(ROOT, 'products', 'fixtures', 'acme', 'locales'), join(directory, 'locales'), {
    recursive: true,
  });
  const source = readFileSync(ACME, 'utf8').replace('"binaryName": "acme"', '"binaryName": "CON"');
  const config = join(directory, 'product.jsonc');
  writeFileSync(config, source, 'utf8');
  assert.throws(
    () => resolveProductDefinition({ rootDir: ROOT, productConfig: config, member: 'cli' }),
    (error) => error instanceof ProductDefinitionError && error.code === 'invalid_binary_name',
  );
});

test('product and data identities reject display names and path fragments', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-product-identity-'));
  cpSync(join(ROOT, 'products', 'fixtures', 'acme', 'locales'), join(directory, 'locales'), {
    recursive: true,
  });
  const source = readFileSync(ACME, 'utf8').replace('"productId": "acme"', '"productId": "Acme/CLI"');
  const config = join(directory, 'product.jsonc');
  writeFileSync(config, source, 'utf8');

  assert.throws(
    () => resolveProductDefinition({ rootDir: ROOT, productConfig: config, member: 'cli' }),
    (error) => error instanceof ProductDefinitionError && error.code === 'invalid_stable_id',
  );
});


test('gui host defaults to tauri and travels into the desktop assembly only', () => {
  const openbitfun = resolveProductDefinition({ rootDir: ROOT, member: 'desktop' });
  assert.equal(openbitfun.assembly.guiHost, 'tauri', 'definitions without the field keep the historical delivery');

  const acmeCli = resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'cli' });
  assert.equal(acmeCli.assembly.guiHost, undefined, 'GUI host is a GUI fact, not a CLI one');
  assert.equal(resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'desktop' }).assembly.guiHost, 'tauri');
});

test('a registered non-default gui host resolves and changes the assembly digest', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-product-gpui-host-'));
  cpSync(join(ROOT, 'products', 'fixtures', 'acme', 'locales'), join(directory, 'locales'), {
    recursive: true,
  });
  const source = readFileSync(ACME, 'utf8').replace('"guiHost": "tauri"', '"guiHost": "gpui-shell"');
  const config = join(directory, 'product.jsonc');
  writeFileSync(config, source, 'utf8');
  try {
    const gpui = resolveProductDefinition({ rootDir: ROOT, productConfig: config, member: 'desktop' });
    assert.equal(gpui.assembly.guiHost, 'gpui-shell');
    assert.notEqual(gpui.assembly.assemblyDigest, resolveProductDefinition({ rootDir: ROOT, productConfig: ACME, member: 'desktop' }).assembly.assemblyDigest);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

test('unregistered or malformed gui host ids fail the build', () => {
  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-product-bad-host-'));
  cpSync(join(ROOT, 'products', 'fixtures', 'acme', 'locales'), join(directory, 'locales'), {
    recursive: true,
  });
  const write = (replacement) => {
    const config = join(directory, `product-${Math.random().toString(36).slice(2)}.jsonc`);
    writeFileSync(config, readFileSync(ACME, 'utf8').replace('"guiHost": "tauri"', replacement), 'utf8');
    return config;
  };
  try {
    assert.throws(
      () => resolveProductDefinition({ rootDir: ROOT, productConfig: write('"guiHost": "ghost-ui"'), member: 'desktop' }),
      (error) => error instanceof ProductDefinitionError && error.code === 'unknown_gui_host',
    );
    assert.throws(
      () => resolveProductDefinition({ rootDir: ROOT, productConfig: write('"guiHost": "GPUI-Shell"'), member: 'desktop' }),
      (error) => error instanceof ProductDefinitionError && error.code === 'invalid_stable_id',
    );
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});
