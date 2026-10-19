import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  compareLeptVersions,
  configureLinuxOcrAbiLinkage,
  debianArchForTarget,
  needsLeptonicaAbiOverride,
  pinnedOcrAbiDigest,
  rewritePkgConfigPrefix,
  verifyPinnedPackage,
} from './prepare-linux-ocr-abi.mjs';

const PKG_CONFIG_RELATIVE_DIR = join('usr', 'lib', 'x86_64-linux-gnu', 'pkgconfig');

function temporaryCacheDir(t) {
  const directory = mkdtempSync(join(tmpdir(), 'openbitfun-ocr-abi-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

function seedReadyPrefix(cacheDir) {
  const pkgConfigDir = join(cacheDir, 'root', PKG_CONFIG_RELATIVE_DIR);
  mkdirSync(pkgConfigDir, { recursive: true });
  writeFileSync(
    join(cacheDir, 'root', '.openbitfun-pinned-ocr-abi.json'),
    `${JSON.stringify({ digest: pinnedOcrAbiDigest() })}\n`,
    'utf8'
  );
  return pkgConfigDir;
}

function leptRun(version) {
  return (command, args) => {
    if (command === 'pkg-config' && args[1] === 'lept' && version) {
      return { status: 0, stdout: `${version}\n`, stderr: '' };
    }
    return { status: 1, stdout: '', stderr: 'not found' };
  };
}

test('Leptonica version comparison orders numerically, not lexically', () => {
  assert.equal(compareLeptVersions('1.82.0', '1.82.0'), 0);
  assert.equal(compareLeptVersions('1.84.1', '1.82.0'), 1);
  assert.equal(compareLeptVersions('1.80.0', '1.82.0'), -1);
  assert.equal(compareLeptVersions('1.9', '1.10'), -1);
  assert.equal(compareLeptVersions('1.82', '1.82.0'), 0);
});

test('The ABI override is required unless the host already provides leptonica 1.82', () => {
  assert.equal(needsLeptonicaAbiOverride('1.82.0'), false);
  assert.equal(needsLeptonicaAbiOverride('1.80.0'), false);
  assert.equal(needsLeptonicaAbiOverride('1.83.0'), true);
  assert.equal(needsLeptonicaAbiOverride('1.84.1'), true);
  assert.equal(needsLeptonicaAbiOverride(null), true);
  assert.equal(needsLeptonicaAbiOverride(undefined), true);
});

test('Debian architecture follows the requested target triple before the host', () => {
  assert.equal(debianArchForTarget('aarch64-unknown-linux-gnu', 'x64'), 'arm64');
  assert.equal(debianArchForTarget('x86_64-unknown-linux-gnu', 'arm64'), 'amd64');
  assert.equal(debianArchForTarget(undefined, 'x64'), 'amd64');
  assert.equal(debianArchForTarget(undefined, 'arm64'), 'arm64');
  assert.equal(debianArchForTarget('riscv64-unknown-linux-gnu', 'x64'), null);
});

test('pkg-config prefix rewriting targets only the prefix line and is idempotent', () => {
  const source = 'prefix=/usr\nexec_prefix=${prefix}\nlibdir=${prefix}/lib\n';
  const rewritten = rewritePkgConfigPrefix(source, '/cache/root');
  assert.equal(rewritten, 'prefix=/cache/root/usr\nexec_prefix=${prefix}\nlibdir=${prefix}/lib\n');
  assert.equal(rewritePkgConfigPrefix(rewritten, '/cache/root'), rewritten);
});

test('Pinned package verification rejects wrong size, wrong digest, and missing files', (t) => {
  const directory = temporaryCacheDir(t);
  const path = join(directory, 'sample.deb');
  const bytes = Buffer.from('pinned-package-bytes');
  writeFileSync(path, bytes);
  const digest = createHash('sha256').update(bytes).digest('hex');

  assert.equal(verifyPinnedPackage(path, { size: bytes.length, sha256: digest }), true);
  assert.equal(verifyPinnedPackage(path, { size: bytes.length + 1, sha256: digest }), false);
  assert.equal(verifyPinnedPackage(path, { size: bytes.length, sha256: '0'.repeat(64) }), false);
  assert.equal(verifyPinnedPackage(join(directory, 'absent.deb'), { size: bytes.length, sha256: digest }), false);
});

test('Non-Linux builds never touch the OCR ABI link', () => {
  const env = {};
  assert.equal(
    configureLinuxOcrAbiLinkage({ env, platform: 'darwin', run: leptRun('1.84.1') }),
    null
  );
  assert.deepEqual(env, {});
});

test('A host already on leptonica 1.82 keeps its own pkg-config path', () => {
  const env = { PKG_CONFIG_PATH: '/host/pc' };
  assert.equal(
    configureLinuxOcrAbiLinkage({
      env,
      platform: 'linux',
      arch: 'x64',
      run: leptRun('1.82.0'),
      download: () => assert.fail('must not download when the host matches'),
    }),
    null
  );
  assert.equal(env.PKG_CONFIG_PATH, '/host/pc');
});

test('A newer host prepends the pinned prefix and preserves the existing search path', (t) => {
  const cacheDir = temporaryCacheDir(t);
  const pkgConfigDir = seedReadyPrefix(cacheDir);
  const env = { PKG_CONFIG_PATH: '/host/pc' };
  assert.equal(
    configureLinuxOcrAbiLinkage({
      env,
      platform: 'linux',
      arch: 'x64',
      cacheDir,
      run: leptRun('1.84.1'),
      download: () => assert.fail('a ready pinned prefix must be reused'),
      log: () => {},
    }),
    pkgConfigDir
  );
  assert.equal(env.PKG_CONFIG_PATH, `${pkgConfigDir}:/host/pc`);
});

test('A host without leptonica at all also gets the pinned prefix', (t) => {
  const cacheDir = temporaryCacheDir(t);
  const pkgConfigDir = seedReadyPrefix(cacheDir);
  const env = {};
  assert.equal(
    configureLinuxOcrAbiLinkage({
      env,
      platform: 'linux',
      arch: 'x64',
      cacheDir,
      run: leptRun(null),
      download: () => assert.fail('a ready pinned prefix must be reused'),
      log: () => {},
    }),
    pkgConfigDir
  );
  assert.equal(env.PKG_CONFIG_PATH, pkgConfigDir);
});

test('An unpinned architecture fails loudly instead of linking the wrong generation', () => {
  assert.throws(
    () =>
      configureLinuxOcrAbiLinkage({
        env: {},
        platform: 'linux',
        arch: 'arm64',
        target: 'aarch64-unknown-linux-gnu',
        run: leptRun('1.84.1'),
        download: () => assert.fail('must not download for an unpinned architecture'),
        log: () => {},
      }),
    /only covers amd64/
  );
});

test('A downloaded package that fails its pinned digest aborts the build', (t) => {
  const cacheDir = temporaryCacheDir(t);
  assert.throws(
    () =>
      configureLinuxOcrAbiLinkage({
        env: {},
        platform: 'linux',
        arch: 'x64',
        cacheDir,
        run: leptRun('1.84.1'),
        download: (url, destination) => writeFileSync(destination, 'tampered'),
        log: () => {},
      }),
    /failed SHA-256 or size verification/
  );
});
