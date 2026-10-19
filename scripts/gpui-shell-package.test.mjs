import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  BIN_NAME,
  PACKAGE_NAME,
  createArchive,
  createDeb,
  debianArchitecture,
  debianControl,
  debianVersion,
  desktopEntry,
  hostTargetTriple,
  workspaceVersion,
} from './gpui-shell-package.mjs';

const repositoryRoot = process.cwd();
const hostTarget = hostTargetTriple();

function temporaryRoot() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'gpui-shell-package-'));
}

// The tests exercise staging around a stand-in executable: packaging itself
// only copies and describes the release binary, so a shell script with the
// expected name proves the layout without a 20-minute release build.
function fakeBinary(root, { version = '1.0.3' } = {}) {
  const binary = path.join(root, BIN_NAME);
  fs.writeFileSync(binary, `#!/bin/sh\necho "${BIN_NAME} ${version}"\n`, 'utf8');
  fs.chmodSync(binary, 0o755);
  return binary;
}

test('workspaceVersion reads the shared workspace version', () => {
  const version = workspaceVersion(repositoryRoot);
  assert.match(version, /^\d+\.\d+\.\d+/);
});

test('debianVersion maps SemVer pre-release dashes to Debian tildes', () => {
  assert.equal(debianVersion('1.0.3'), '1.0.3');
  assert.equal(debianVersion('1.0.3-beta.2'), '1.0.3~beta.2');
});

test('debianArchitecture maps supported Rust targets only', () => {
  assert.equal(debianArchitecture('x86_64-unknown-linux-gnu'), 'amd64');
  assert.equal(debianArchitecture('aarch64-unknown-linux-gnu'), 'arm64');
  assert.throws(() => debianArchitecture('x86_64-apple-darwin'), /Unsupported Debian architecture/);
});

test('debianControl carries the identity, architecture, and dependencies', () => {
  const control = debianControl({
    version: '1.0.3',
    architecture: 'amd64',
    depends: ['libc6', 'libgcc-s1', 'libx11-6'],
    installedSizeKb: 42000,
  });
  assert.match(control, new RegExp(`^Package: ${PACKAGE_NAME}$`, 'm'));
  assert.match(control, /^Version: 1\.0\.3$/m);
  assert.match(control, /^Architecture: amd64$/m);
  assert.match(control, /^Depends: libc6, libgcc-s1, libx11-6$/m);
  assert.match(control, /^Installed-Size: 42000$/m);
  assert.match(control, /^Maintainer: .+@.+$/m);
  assert.ok(control.endsWith('\n'));
});

test('desktopEntry launches the packaged binary and icon', () => {
  const entry = desktopEntry();
  assert.match(entry, new RegExp(`^Exec=${BIN_NAME}$`, 'm'));
  assert.match(entry, new RegExp(`^Icon=${PACKAGE_NAME}$`, 'm'));
  assert.match(entry, /^Type=Application$/m);
  assert.match(entry, /^Terminal=false$/m);
});

test('createArchive stages the binary with license files and checksums it', () => {
  const root = temporaryRoot();
  const outputDir = path.join(root, 'out');
  const stagingRoot = path.join(root, 'stage');
  fs.mkdirSync(outputDir, { recursive: true });
  fs.mkdirSync(stagingRoot, { recursive: true });
  const binary = fakeBinary(root);

  const { archivePath, stageDir } = createArchive({
    binary,
    version: '1.0.3',
    target: hostTarget,
    outputDir,
    stagingRoot,
  });

  assert.ok(fs.existsSync(archivePath));
  assert.ok(fs.existsSync(path.join(stageDir, BIN_NAME)));
  assert.ok(fs.existsSync(path.join(stageDir, 'LICENSE')));
  const checksum = fs.readFileSync(`${archivePath}.sha256`, 'utf8');
  assert.match(checksum, new RegExp(`^[0-9a-f]{64}  ${PACKAGE_NAME}-1\\.0\\.3-${hostTarget}\\.tar\\.gz\\n$`));
});

test('createDeb builds a package that unpacks into a runnable tree', { skip: process.platform !== 'linux' }, () => {
  const root = temporaryRoot();
  const outputDir = path.join(root, 'out');
  const stagingRoot = path.join(root, 'stage');
  fs.mkdirSync(outputDir, { recursive: true });
  fs.mkdirSync(stagingRoot, { recursive: true });
  const binary = fakeBinary(root);

  const { debPath, tree } = createDeb({ binary, version: '1.0.3', target: hostTarget, outputDir, stagingRoot });

  assert.ok(fs.existsSync(debPath));
  assert.ok(fs.existsSync(path.join(tree, 'usr/bin', BIN_NAME)));
  assert.ok(fs.existsSync(path.join(tree, 'usr/share/applications', `${PACKAGE_NAME}.desktop`)));
  assert.ok(fs.existsSync(path.join(tree, 'usr/share/icons/hicolor/256x256/apps', `${PACKAGE_NAME}.png`)));
  assert.ok(fs.existsSync(path.join(tree, 'DEBIAN/control')));

  const listing = spawnSync('dpkg-deb', ['-c', debPath], { encoding: 'utf8' });
  assert.equal(listing.status, 0);
  assert.ok(listing.stdout.includes(`./usr/bin/${BIN_NAME}`));
  assert.ok(listing.stdout.includes('./usr/share/icons/hicolor/256x256/apps/'));
});
