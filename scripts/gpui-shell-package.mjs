#!/usr/bin/env node
// Packages the GPUI shell as a standalone binary, a portable tar.gz archive,
// and a Debian package.
//
// The shell has no webview assets and embeds its icon set at compile time, so
// the release artifact is a single executable: everything here stages files
// around that one binary. The tar.gz mirrors the CLI release archive shape
// (`<name>-<version>-<target>.tar.gz` plus a `.sha256`), which is the payload a
// future self-update feed consumes.

import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

import { extractProductConfigArg } from './product-customization/cli.mjs';
import { resolveProductDefinition } from './product-customization/resolver.mjs';

const ROOT = resolve(import.meta.dirname, '..');
export const BIN_NAME = 'openbitfun-gpui-shell';
export const PACKAGE_NAME = 'openbitfun-gpui-shell';
const ICON_SIZES = [16, 32, 48, 64, 128, 256];
// `dpkg-shlibdeps` only sees DT_NEEDED. Verified against a running release
// binary (`/proc/<pid>/maps`): the wgpu Vulkan backend loads the loader at
// runtime, and the GL fallback plus Wayland sessions load EGL/Wayland the same
// way. The Vulkan loader is the primary renderer's hard requirement; without a
// driver ICD next to it nothing paints, which is why `mesa-vulkan-drivers`
// (llvmpipe software rendering) is a recommendation rather than a dependency.
const RUNTIME_DEPENDS = ['libvulkan1'];
const RUNTIME_RECOMMENDS = ['mesa-vulkan-drivers', 'libegl1', 'libwayland-client0'];
/** Used only when `dpkg-shlibdeps` cannot run on the packaging host. */
const SHLIBS_FALLBACK = ['libc6', 'libgcc-s1'];

export function workspaceVersion(root = ROOT) {
  const manifest = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const version = manifest.match(/^\[workspace\.package\][\s\S]*?^version = "([^"]+)"/m)?.[1];
  if (!version || !/^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?$/.test(version)) {
    throw new Error('Could not resolve a valid workspace package version');
  }
  return version;
}

/** Debian versions cannot carry SemVer pre-release dashes. */
export function debianVersion(version) {
  return version.replace(/-/g, '~');
}

export function debianArchitecture(target) {
  if (!/-linux-/.test(target)) throw new Error(`Unsupported Debian architecture for target ${target}`);
  if (target.startsWith('x86_64-')) return 'amd64';
  if (target.startsWith('aarch64-')) return 'arm64';
  throw new Error(`Unsupported Debian architecture for target ${target}`);
}

export function hostTargetTriple() {
  const result = spawnSync('rustc', ['-vV'], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error('rustc -vV failed; a Rust toolchain is required');
  const triple = result.stdout.match(/^host: (.+)$/m)?.[1]?.trim();
  if (!triple) throw new Error('Could not read the host target triple from rustc');
  return triple;
}

export function debianControl({ version, architecture, depends, recommends, installedSizeKb }) {
  return [
    `Package: ${PACKAGE_NAME}`,
    `Version: ${debianVersion(version)}`,
    'Section: utils',
    'Priority: optional',
    `Architecture: ${architecture}`,
    `Depends: ${depends.join(', ')}`,
    ...(recommends?.length ? [`Recommends: ${recommends.join(', ')}`] : []),
    `Installed-Size: ${installedSizeKb}`,
    'Maintainer: OpenBitFun Team <team@openbitfun.com>',
    'Homepage: https://openbitfun.com',
    `Description: OpenBitFun GPUI desktop shell`,
    ' Native GPU-rendered desktop frontend for OpenBitFun. Experimental: the',
    ' Tauri desktop app remains the default entry point.',
    '',
  ].join('\n');
}

export function desktopEntry() {
  return [
    '[Desktop Entry]',
    'Type=Application',
    'Name=OpenBitFun GPUI Shell',
    'Comment=Native GPU-rendered desktop shell (experimental)',
    `Exec=${BIN_NAME}`,
    `Icon=${PACKAGE_NAME}`,
    'Terminal=false',
    'Categories=Development;Utility;',
    'StartupNotify=true',
    '',
  ].join('\n');
}

export function resolveTargetPath(root, target, relative) {
  return target === hostTargetTriple()
    ? resolve(root, 'target', relative)
    : resolve(root, 'target', target, relative);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', windowsHide: true, ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${result.status}`);
}

function capture(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', windowsHide: true, ...options });
  if (result.error) throw result.error;
  return result;
}

function sha256File(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

/**
 * Real library dependencies, asked of the same tool the Debian archive uses.
 * `dpkg-shlibdeps` insists on a `debian/control` beside it even for `-O`, so a
 * throwaway one is staged next to the binary being inspected.
 */
export function shlibDepends(binary, { stagingRoot }) {
  const debianDir = join(stagingRoot, 'debian');
  rmSync(debianDir, { recursive: true, force: true });
  mkdirSync(debianDir, { recursive: true });
  writeFileSync(join(debianDir, 'control'), [
    `Source: ${PACKAGE_NAME}`,
    'Section: utils',
    'Priority: optional',
    'Maintainer: OpenBitFun Team <team@openbitfun.com>',
    'Standards-Version: 4.6.0',
    '',
    `Package: ${PACKAGE_NAME}`,
    'Architecture: any',
    'Depends: ${shlibs:Depends}',
    'Description: OpenBitFun GPUI desktop shell',
    '',
  ].join('\n'), 'utf8');
  try {
    const result = capture('dpkg-shlibdeps', ['-O', `-e${binary}`], { cwd: stagingRoot });
    if (result.status !== 0) return null;
    const depends = result.stdout.match(/^shlibs:Depends=(.+)$/m)?.[1];
    return depends ? depends.split(',').map(entry => entry.trim()).filter(Boolean) : null;
  } catch {
    return null;
  } finally {
    rmSync(debianDir, { recursive: true, force: true });
  }
}

function stageIcons(stagingRoot) {
  for (const size of ICON_SIZES) {
    const source = resolve(ROOT, 'assets/brand/exports', `openbitfun-app-icon-${size}.png`);
    const target = join(stagingRoot, 'usr/share/icons/hicolor', `${size}x${size}`, 'apps', `${PACKAGE_NAME}.png`);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(source, target);
  }
}

export function createDeb({ binary, version, target, outputDir, stagingRoot }) {
  const architecture = debianArchitecture(target);
  const tree = join(stagingRoot, `${PACKAGE_NAME}_${debianVersion(version)}_${architecture}`);
  rmSync(tree, { recursive: true, force: true });

  const installedBinary = join(tree, 'usr/bin', BIN_NAME);
  mkdirSync(join(tree, 'usr/bin'), { recursive: true });
  copyFileSync(binary, installedBinary);
  chmodSync(installedBinary, 0o755);

  mkdirSync(join(tree, 'usr/share/applications'), { recursive: true });
  writeFileSync(join(tree, 'usr/share/applications', `${PACKAGE_NAME}.desktop`), desktopEntry(), 'utf8');
  stageIcons(tree);

  mkdirSync(join(tree, 'usr/share/doc', PACKAGE_NAME), { recursive: true });
  const license = resolve(ROOT, 'LICENSE');
  if (statSync(license, { throwIfNoEntry: false })) {
    copyFileSync(license, join(tree, 'usr/share/doc', PACKAGE_NAME, 'copyright'));
  }

  const depends = [...(shlibDepends(installedBinary, { stagingRoot }) ?? SHLIBS_FALLBACK), ...RUNTIME_DEPENDS];
  const installedSizeKb = Math.ceil(statSync(installedBinary).size / 1024);
  mkdirSync(join(tree, 'DEBIAN'), { recursive: true });
  writeFileSync(
    join(tree, 'DEBIAN/control'),
    debianControl({ version, architecture, depends, recommends: RUNTIME_RECOMMENDS, installedSizeKb }),
    'utf8',
  );

  const debPath = join(outputDir, `${PACKAGE_NAME}_${debianVersion(version)}_${architecture}.deb`);
  rmSync(debPath, { force: true });
  run('dpkg-deb', ['--build', '--root-owner-group', tree, debPath]);
  return { debPath, tree, architecture, depends, recommends: RUNTIME_RECOMMENDS };
}

export function createArchive({ binary, version, target, outputDir, stagingRoot }) {
  const stageName = `${PACKAGE_NAME}-${version}-${target}`;
  const stageDir = join(stagingRoot, stageName);
  rmSync(stageDir, { recursive: true, force: true });
  mkdirSync(stageDir, { recursive: true });
  const stagedBinary = join(stageDir, BIN_NAME);
  copyFileSync(binary, stagedBinary);
  chmodSync(stagedBinary, 0o755);
  for (const file of ['LICENSE', 'THIRD_PARTY_NOTICES.md']) {
    const source = resolve(ROOT, file);
    if (statSync(source, { throwIfNoEntry: false })) copyFileSync(source, join(stageDir, file));
  }
  const readme = resolve(ROOT, 'src/apps/gpui-shell/README.md');
  if (statSync(readme, { throwIfNoEntry: false })) copyFileSync(readme, join(stageDir, 'README.md'));

  const archivePath = join(outputDir, `${stageName}.tar.gz`);
  rmSync(archivePath, { force: true });
  run('tar', ['-C', stagingRoot, '-czf', archivePath, stageName]);
  writeFileSync(`${archivePath}.sha256`, `${sha256File(archivePath)}  ${stageName}.tar.gz\n`, 'utf8');
  return { archivePath, stageDir };
}

function verifyBinary(binary, version) {
  const result = capture(binary, ['--version']);
  const text = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
  if (result.status !== 0 || !text.includes(version)) {
    throw new Error(`${BIN_NAME} --version did not report ${version}: ${text}`);
  }
  return text;
}

function verifyDeb(debPath, version, workRoot) {
  const info = capture('dpkg-deb', ['-I', debPath]);
  if (info.status !== 0 || !info.stdout.includes(`Version: ${debianVersion(version)}`)) {
    throw new Error(`dpkg-deb -I did not report the packaged version for ${debPath}`);
  }
  const listing = capture('dpkg-deb', ['-c', debPath]);
  for (const required of [
    `./usr/bin/${BIN_NAME}`,
    `./usr/share/applications/${PACKAGE_NAME}.desktop`,
    './usr/share/icons/hicolor/256x256/apps/',
  ]) {
    if (!listing.stdout.includes(required)) {
      throw new Error(`Debian package is missing ${required}`);
    }
  }
  const unpacked = join(workRoot, 'unpacked');
  rmSync(unpacked, { recursive: true, force: true });
  mkdirSync(unpacked, { recursive: true });
  run('dpkg-deb', ['-x', debPath, unpacked]);
  const unpackedBinary = join(unpacked, 'usr/bin', BIN_NAME);
  return verifyBinary(unpackedBinary, version);
}

function parseArgs(argv) {
  const options = { build: true, target: null, outputDir: null, productConfig: null };
  const forwarded = [];
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--no-build') options.build = false;
    else if (arg === '--target') options.target = argv[++index];
    else if (arg.startsWith('--target=')) options.target = arg.slice('--target='.length);
    else if (arg === '--output') options.outputDir = argv[++index];
    else if (arg.startsWith('--output=')) options.outputDir = arg.slice('--output='.length);
    else forwarded.push(arg);
  }
  const extracted = extractProductConfigArg(forwarded);
  options.productConfig = extracted.productConfig;
  if (extracted.forwardArgs.length > 0) {
    throw new Error(`Unknown argument: ${extracted.forwardArgs[0]}`);
  }
  return options;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const version = workspaceVersion();
  const target = options.target ?? hostTargetTriple();
  const outputDir = resolve(options.outputDir ?? join(ROOT, 'target/gpui-shell-package'));
  const workRoot = join(outputDir, '.work');
  mkdirSync(outputDir, { recursive: true });
  rmSync(workRoot, { recursive: true, force: true });
  mkdirSync(workRoot, { recursive: true });

  // The product definition picks the default GUI host (blueprint §7). While a
  // different host is the default, this archive is the parallel (non-default)
  // delivery, which is fine — say so instead of pretending otherwise.
  const guiHost = resolveProductDefinition({
    rootDir: ROOT,
    productConfig: options.productConfig ?? undefined,
    member: 'desktop',
  }).assembly.guiHost;
  if (guiHost !== 'gpui-shell') {
    console.warn(
      `[gpui-shell-package] product definition selects GUI host "${guiHost}"; packaging this archive as the non-default delivery`,
    );
  }

  if (options.build) {
    const cargoArgs = ['build', '--release', '-p', PACKAGE_NAME];
    if (target !== hostTargetTriple()) cargoArgs.push('--target', target);
    run('cargo', cargoArgs, { cwd: ROOT });
  }

  const binary = resolveTargetPath(ROOT, target, join('release', BIN_NAME));
  if (!statSync(binary, { throwIfNoEntry: false })) {
    throw new Error(`Release binary not found at ${binary}; run without --no-build first`);
  }
  const reported = verifyBinary(binary, version);
  console.log(`[gpui-shell-package] binary reports: ${reported}`);

  const { archivePath, stageDir } = createArchive({ binary, version, target, outputDir, stagingRoot: workRoot });
  console.log(`[gpui-shell-package] archive: ${archivePath} (${statSync(archivePath).size} bytes)`);

  let deb = null;
  if (process.platform === 'linux') {
    deb = createDeb({ binary, version, target, outputDir, stagingRoot: workRoot });
    const unpackedReport = verifyDeb(deb.debPath, version, workRoot);
    console.log(`[gpui-shell-package] deb: ${deb.debPath} (${statSync(deb.debPath).size} bytes)`);
    console.log(`[gpui-shell-package] depends: ${deb.depends.join(', ')}`);
    console.log(`[gpui-shell-package] unpacked binary reports: ${unpackedReport}`);
  } else {
    console.log('[gpui-shell-package] deb packaging is Linux-only; archive only on this host');
  }
  console.log(`[gpui-shell-package] staged tree: ${stageDir}`);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  main();
}
