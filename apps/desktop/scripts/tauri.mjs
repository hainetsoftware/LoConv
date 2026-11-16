import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { constants, createReadStream } from 'node:fs';
import { access, lstat, mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';

const stateDir = new URL('../../../.loconv/', import.meta.url);
const config = new URL('verification-tools.json', stateDir);
const toolKeys = ['LOCONV_DEV_FFMPEG', 'LOCONV_DEV_FFPROBE'];
const [mode, ...args] = process.argv.slice(2);

async function executablePin(path) {
  const executable = await realpath(path);
  if (!(await lstat(executable)).isFile()) {
    throw new Error('A media tool must resolve to a regular executable.');
  }
  await access(executable, constants.X_OK);
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(executable)) {
    hash.update(bytes);
  }
  return { executable, digest: hash.digest('hex') };
}

async function saveToolPins(paths) {
  if (paths.length !== toolKeys.length || paths.some((path) => !isAbsolute(path))) {
    throw new Error('Usage: npm run tauri:setup-media -- /absolute/path/to/ffmpeg /absolute/path/to/ffprobe');
  }
  const pins = {};
  for (const [index, key] of toolKeys.entries()) {
    const { executable, digest } = await executablePin(paths[index]);
    pins[key] = executable;
    pins[key + '_SHA256'] = digest;
  }

  await mkdir(stateDir, { recursive: true, mode: 0o700 });
  try {
    await writeFile(config, JSON.stringify(pins, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    throw new Error('Media tools are already configured. Review and update .loconv/verification-tools.json to replace them.');
  }
  console.log('Media tools configured in .loconv/verification-tools.json. Run npm run tauri dev to start LoConv.');
}

async function pinnedTools() {
  let pins;
  try {
    pins = JSON.parse(await readFile(config, 'utf8'));
  } catch (error) {
    if (error.code !== 'ENOENT') {
      throw new Error('Could not read .loconv/verification-tools.json.');
    }
    const hasToolSettings = toolKeys.some((key) => key in process.env || key + '_SHA256' in process.env);
    if (!hasToolSettings) return null;
    pins = process.env;
  }

  const env = { ...process.env, LOCONV_ENABLE_DEV_TOOLS: '1' };
  for (const key of toolKeys) {
    const path = pins?.[key];
    const sha256 = pins?.[key + '_SHA256'];
    if (typeof path !== 'string' || !isAbsolute(path) ||
        typeof sha256 !== 'string' || !/^[a-f0-9]{64}$/i.test(sha256)) {
      throw new Error('Set an absolute path and a SHA-256 pin for ' + key + ' in .loconv/verification-tools.json.');
    }
    const { executable, digest } = await executablePin(path);
    if (digest !== sha256.toLowerCase()) {
      throw new Error('The SHA-256 pin for ' + key + ' no longer matches. Review the executable and update the pin.');
    }
    env[key] = executable;
    env[key + '_SHA256'] = sha256;
  }
  return env;
}

function runTauri(args, env) {
  const cli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url));
  const child = spawn(process.execPath, [cli, ...args], {
    cwd: fileURLToPath(new URL('../', import.meta.url)),
    env,
    stdio: 'inherit',
  });
  child.on('error', error => {
    console.error('LoConv: ' + error.message);
    process.exitCode = 1;
  });
  child.on('exit', (code, signal) => {
    process.exitCode = code ?? (signal === 'SIGINT' ? 130 : 1);
  });
}

try {
  if (mode === '--setup-media') {
    if (process.platform !== 'darwin') throw new Error('Media development mode requires macOS.');
    await saveToolPins(args);
  } else if (mode === '--media' || (mode === 'dev' && process.platform === 'darwin' && !args.includes('--release'))) {
    if (process.platform !== 'darwin' || args.includes('--release')) {
      throw new Error('Media development tools require a macOS debug build.');
    }
    const env = await pinnedTools();
    if (env) {
      runTauri(['dev', '--features', 'dev-tools', ...args], env);
    } else if (mode === '--media') {
      throw new Error('Configure FFmpeg and FFprobe first with npm run tauri:setup-media.');
    } else {
      console.warn('LoConv is starting with image support. To enable audio and video, run npm run tauri:setup-media first.');
      runTauri(['dev', ...args], process.env);
    }
  } else {
    runTauri(process.argv.slice(2), process.env);
  }
} catch (error) {
  console.error('LoConv: ' + error.message);
  process.exitCode = 1;
}
