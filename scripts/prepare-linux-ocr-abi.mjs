import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_CACHE_DIR = join(ROOT, 'node_modules', '.cache', 'openbitfun-linux-ocr-abi');
const DEFAULT_MIRROR = 'https://deb.debian.org/debian';
const MARKER_FILE = '.openbitfun-pinned-ocr-abi.json';
const PKG_CONFIG_RELATIVE_DIR = join('usr', 'lib', 'x86_64-linux-gnu', 'pkgconfig');

// The desktop deb declares its leptonica runtime dependency by package name
// (`bundle.linux.deb.depends` in src/apps/desktop/tauri.conf.json). That name
// pins the ABI generation the packaged binary has to link: `liblept5` ships
// liblept.so.5 (leptonica 1.82), `libleptonica6` ships libleptonica.so.6
// (1.84). pkg-config otherwise follows whatever the build host provides, so a
// newer host (Deepin 25 / Debian 13 ship leptonica 1.84) silently emits a deb
// that installs on a 1.82 target and then cannot start:
//   openbitfun-desktop: error while loading shared libraries: libleptonica.so.6
// The pinned Debian 12 set below is the 1.82 generation, so the artifact links
// what the declaration promises. Keeping tesseract in the same set matters for
// headers: tesseract.pc has `Requires.private: lept`, so bindgen must not fall
// back to a host leptonica of a different generation.
const HOST_LEPT_CEILING = '1.82.0';
const PINNED_DEBIAN_ARCH = 'amd64';
const PINNED_OCR_PACKAGES = [
  {
    name: 'libleptonica-dev',
    directory: 'pool/main/l/leptonlib',
    file: 'libleptonica-dev_1.82.0-3+b3_amd64.deb',
    size: 1466120,
    sha256: '373cf20cec152895702c2903208bc823c671b6f0953e1b52e72ee7259554b218',
  },
  {
    name: 'liblept5',
    directory: 'pool/main/l/leptonlib',
    file: 'liblept5_1.82.0-3+b3_amd64.deb',
    size: 1049560,
    sha256: 'cddc18cffc38bb1839f2b5336822646b34dafc5dd40468b658d26c20e0eb97af',
  },
  {
    name: 'libtesseract-dev',
    directory: 'pool/main/t/tesseract',
    file: 'libtesseract-dev_5.3.0-2_amd64.deb',
    size: 1494960,
    sha256: 'a899cc7c6c7af8c2a5488f886483f29c730389dcf97a5a614b9df1a72348b7fa',
  },
  {
    name: 'libtesseract5',
    directory: 'pool/main/t/tesseract',
    file: 'libtesseract5_5.3.0-2_amd64.deb',
    size: 1278876,
    sha256: '19b1719f30235eaf209866b219d4dbdd4dddd813758a8d5b9715c78c40702ff1',
  },
];

const TRIPLE_ARCH_TO_DEBIAN = { x86_64: 'amd64', aarch64: 'arm64' };
const HOST_ARCH_TO_DEBIAN = { x64: 'amd64', arm64: 'arm64' };

function runCommand(command, args) {
  return spawnSync(command, args, { encoding: 'utf8', windowsHide: true, shell: false });
}

export function compareLeptVersions(left, right) {
  const leftParts = String(left).split('.').map(Number);
  const rightParts = String(right).split('.').map(Number);
  for (let index = 0; index < Math.max(leftParts.length, rightParts.length); index += 1) {
    const difference = (leftParts[index] || 0) - (rightParts[index] || 0);
    if (difference !== 0) return difference > 0 ? 1 : -1;
  }
  return 0;
}

export function needsLeptonicaAbiOverride(hostVersion) {
  if (!hostVersion) return true;
  return compareLeptVersions(hostVersion, HOST_LEPT_CEILING) > 0;
}

export function debianArchForTarget(target, hostArch = process.arch) {
  if (target) {
    return TRIPLE_ARCH_TO_DEBIAN[String(target).split('-')[0]] || null;
  }
  return HOST_ARCH_TO_DEBIAN[hostArch] || null;
}

export function rewritePkgConfigPrefix(content, prefixRoot) {
  return content.replace(/^prefix=\/usr$/m, `prefix=${prefixRoot}/usr`);
}

export function pinnedOcrAbiDigest() {
  return createHash('sha256').update(JSON.stringify(PINNED_OCR_PACKAGES)).digest('hex');
}

export function verifyPinnedPackage(path, pkg) {
  if (!existsSync(path)) return false;
  const bytes = readFileSync(path);
  return (
    bytes.length === pkg.size && createHash('sha256').update(bytes).digest('hex') === pkg.sha256
  );
}

export function probeHostLeptVersion(run = runCommand) {
  const result = run('pkg-config', ['--modversion', 'lept']);
  if (result.error || result.status !== 0) return null;
  const version = String(result.stdout || '').trim();
  return version || null;
}

function downloadFile(url, destination) {
  const result = runCommand(process.platform === 'win32' ? 'curl.exe' : 'curl', [
    '--fail',
    '--location',
    '--silent',
    '--show-error',
    '--proto',
    '=https',
    '--proto-redir',
    '=https',
    '--retry',
    '3',
    '--connect-timeout',
    '20',
    '--max-time',
    '180',
    '--output',
    destination,
    url,
  ]);
  if (result.error || result.status !== 0) {
    const detail = result.error?.message || result.stderr || `exit status ${result.status}`;
    throw new Error(
      `Failed to download ${url}: ${detail}\n` +
        'Set OPENBITFUN_DEBIAN_MIRROR to a reachable Debian mirror, for example\n' +
        '  OPENBITFUN_DEBIAN_MIRROR=https://mirrors.tuna.tsinghua.edu.cn/debian'
    );
  }
}

function prefixIsReady(cacheDir) {
  try {
    const marker = JSON.parse(readFileSync(join(cacheDir, 'root', MARKER_FILE), 'utf8'));
    return marker.digest === pinnedOcrAbiDigest();
  } catch {
    return false;
  }
}

function rewritePkgConfigFiles(prefixRoot) {
  const directory = join(prefixRoot, PKG_CONFIG_RELATIVE_DIR);
  if (!existsSync(directory)) {
    throw new Error(`Pinned OCR packages did not provide ${PKG_CONFIG_RELATIVE_DIR}`);
  }
  for (const entry of readdirSync(directory)) {
    if (!entry.endsWith('.pc')) continue;
    const path = join(directory, entry);
    writeFileSync(path, rewritePkgConfigPrefix(readFileSync(path, 'utf8'), prefixRoot), 'utf8');
  }
}

function materializePrefix(cacheDir, { mirror, download, run, log }) {
  const prefixRoot = join(cacheDir, 'root');
  const downloads = join(cacheDir, 'downloads');
  rmSync(prefixRoot, { recursive: true, force: true });
  rmSync(downloads, { recursive: true, force: true });
  mkdirSync(downloads, { recursive: true });
  for (const pkg of PINNED_OCR_PACKAGES) {
    const destination = join(downloads, pkg.file);
    log(`[linux-ocr-abi] fetching pinned ${pkg.name} ${pkg.file}`);
    download(`${mirror}/${pkg.directory}/${pkg.file}`, destination);
    if (!verifyPinnedPackage(destination, pkg)) {
      throw new Error(`Pinned OCR package failed SHA-256 or size verification: ${pkg.file}`);
    }
    const extracted = run('dpkg-deb', ['-x', destination, prefixRoot]);
    if (extracted.error || extracted.status !== 0) {
      const detail = extracted.error?.message || extracted.stderr || `exit status ${extracted.status}`;
      throw new Error(`Failed to extract ${pkg.file} (is dpkg-deb installed?): ${detail}`);
    }
  }
  rewritePkgConfigFiles(prefixRoot);
  writeFileSync(
    join(prefixRoot, MARKER_FILE),
    `${JSON.stringify({ digest: pinnedOcrAbiDigest() }, null, 2)}\n`,
    'utf8'
  );
  rmSync(downloads, { recursive: true, force: true });
}

// Returns the pkg-config directory to prepend, or null when the host already
// provides the generation the deb declares.
export function configureLinuxOcrAbiLinkage(options = {}) {
  const {
    target,
    env = process.env,
    platform = process.platform,
    arch = process.arch,
    run = runCommand,
    download = downloadFile,
    log = console.log,
    cacheDir = DEFAULT_CACHE_DIR,
    mirror = env.OPENBITFUN_DEBIAN_MIRROR || DEFAULT_MIRROR,
  } = options;
  if (platform !== 'linux') return null;

  const hostVersion = probeHostLeptVersion(run);
  if (!needsLeptonicaAbiOverride(hostVersion)) {
    log(`[linux-ocr-abi] host leptonica ${hostVersion} matches the declared liblept5 dependency`);
    return null;
  }

  const debianArch = debianArchForTarget(target, arch);
  if (debianArch !== PINNED_DEBIAN_ARCH) {
    throw new Error(
      `Host leptonica ${hostVersion || 'is missing'} does not match the declared liblept5 ` +
        `dependency, and the pinned OCR package set only covers ${PINNED_DEBIAN_ARCH} ` +
        `(this build is ${debianArch || 'unknown'}). Build the deb on a leptonica 1.82 host, ` +
        'or extend PINNED_OCR_PACKAGES in scripts/prepare-linux-ocr-abi.mjs.'
    );
  }

  if (!prefixIsReady(cacheDir)) {
    materializePrefix(cacheDir, { mirror, download, run, log });
  }
  const pkgConfigDir = join(cacheDir, 'root', PKG_CONFIG_RELATIVE_DIR);
  env.PKG_CONFIG_PATH = env.PKG_CONFIG_PATH
    ? `${pkgConfigDir}:${env.PKG_CONFIG_PATH}`
    : pkgConfigDir;
  log(
    `[linux-ocr-abi] host leptonica ${hostVersion || 'is missing'}; linking the pinned Debian 12 ` +
      'leptonica 1.82 set so the deb matches its declared liblept5 dependency'
  );
  return pkgConfigDir;
}
