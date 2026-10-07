import { spawn, execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, mkdirSync, openSync, closeSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { createServer } from 'node:net';
import { once } from 'node:events';

export default async function setup() {
  const root = resolve(import.meta.dirname, '..');
  const binary = resolve(root, 'server/target/debug/servicehub');
  const dist = resolve(root, 'web/dist');
  if (!existsSync(binary)) execFileSync('cargo', ['build'], { cwd: resolve(root, 'server'), stdio: 'inherit' });
  if (!existsSync(resolve(dist, 'index.html'))) {
    if (!existsSync(resolve(root, 'web/node_modules'))) execFileSync('npm', ['ci'], { cwd: resolve(root, 'web'), stdio: 'inherit' });
    execFileSync('npm', ['run', 'build'], { cwd: resolve(root, 'web'), stdio: 'inherit' });
  }
  const socket = createServer();
  socket.listen(0, '127.0.0.1');
  await once(socket, 'listening');
  const port = (socket.address() as { port: number }).port;
  await new Promise<void>((done) => socket.close(() => done()));
  const data = mkdtempSync(resolve(tmpdir(), 'servicehub-e2e-'));
  const baseURL = `http://127.0.0.1:${port}`;
  const env = { ...process.env, PORT: String(port), DATA_DIR: data, WEB_DIST: dist,
    SEED_DATA_DIR: resolve(root, 'server/seed-data'), DEMO_MODE: 'true', DEMO_RESET_HOURS: '0',
    DEMO_ENDS_AT: '', COOKIE_SECURE: 'false', TRUST_PROXY: 'false',
    PUBLIC_BASE_URL: baseURL, INTERNAL_BASE_URL: baseURL };
  mkdirSync(resolve(root, 'e2e/test-results'), { recursive: true });
  const log = openSync(resolve(root, 'e2e/test-results/server.log'), 'w');
  let child: ReturnType<typeof spawn> | undefined;
  const teardown = async () => {
    if (child && child.exitCode === null) {
      const exited = once(child, 'exit');
      child.kill('SIGTERM');
      const kill = setTimeout(() => child?.kill('SIGKILL'), 5000);
      await exited;
      clearTimeout(kill);
    }
    closeSync(log);
    rmSync(data, { recursive: true, force: true });
  };
  try {
    execFileSync(binary, ['seed-demo'], { cwd: root, env, stdio: ['ignore', log, log], timeout: 120000 });
    child = spawn(binary, ['serve'], { cwd: root, env, stdio: ['ignore', log, log] });
    let ready = false;
    for (let attempt = 0; attempt < 200; attempt++) {
      if (child.exitCode !== null) throw new Error('servicehub exited; see test-results/server.log');
      try { ready = (await fetch(`${baseURL}/api/health`)).ok; } catch { /* Starting. */ }
      if (ready) break;
      await new Promise((done) => setTimeout(done, 100));
    }
    if (!ready) throw new Error('servicehub did not become healthy; see test-results/server.log');
    process.env.E2E_BASE_URL = baseURL;
    return teardown;
  } catch (error) { await teardown(); throw error; }
}
