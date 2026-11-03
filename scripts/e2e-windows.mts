// Drives the real Termix window on Windows through WebView2's debugging port,
// against the local test SSH server. Also runs the CLI against the same server.
//
//   npm --prefix app ci && npm --prefix app run build:e2e
//   cargo build -p termix-cli -p termix-app
//   cargo build -p termix-session --example test_server
//   node scripts/e2e-windows.mts [screenshot-folder]
//
// Nothing touches your real Termix data: everything uses a temporary folder.

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { windowTests } from "./e2e-window-tests.mts";

const root = resolve(import.meta.dirname, "..");
const bin = (name: string) => join(root, "target", "debug", name);
const shots = process.argv[2];
if (shots) mkdirSync(shots, { recursive: true });
const work = mkdtempSync(join(tmpdir(), "termix-e2e-"));
const debugPort = 9333;
const failures: string[] = [];
let passed = 0;

function check(name: string, condition: unknown, detail = ""): void {
  if (condition) {
    passed += 1;
    console.log(`ok    ${name}`);
  } else {
    failures.push(name);
    console.log(`FAIL  ${name} ${detail}`);
  }
}

const sleep = (ms: number) => new Promise<void>((done) => setTimeout(done, ms));

async function until<T>(what: string, read: () => T | Promise<T>, timeout = 20000): Promise<NonNullable<T>> {
  const started = Date.now();
  for (;;) {
    const value = await read();
    if (value) return value;
    if (Date.now() - started > timeout) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
}

// --- test server

const server = spawn(bin("examples/test_server.exe"), { stdio: ["ignore", "pipe", "inherit"] });
let serverOutput = "";
server.stdout.on("data", (chunk) => (serverOutput += chunk));
const info = await until("the test server", () => {
  const port = /port (\d+)/.exec(serverOutput);
  const fingerprint = /fingerprint (\S+)/.exec(serverOutput);
  const password = /password (\S+)/.exec(serverOutput);
  const files = /files (.+)/.exec(serverOutput);
  return port && fingerprint && password && files
    ? { port: port[1], fingerprint: fingerprint[1], password: password[1], files: files[1].trim() }
    : null;
});

// --- CLI

function cli(args: string[], { password = true } = {}) {
  const env = { ...process.env };
  delete env.TERMIX_PASSWORD;
  if (password) env.TERMIX_PASSWORD = info.password;
  return spawnSync(bin("termix.exe"), ["--data-dir", join(work, "cli"), ...args], { env, encoding: "utf8" });
}

function cliTests() {
  check("cli: add machine", cli(["machine", "add", "--name", "box", "--host", "127.0.0.1", "--port", info.port, "--username", "ops"]).status === 0);
  const unknown = cli(["exec", "box", "uname"]);
  check("cli: unknown host refused and fingerprint shown", unknown.status !== 0 && unknown.stderr.includes(info.fingerprint), unknown.stderr);
  const wrong = cli(["exec", "box", "--trust", "SHA256:wrong", "uname"]);
  check("cli: wrong fingerprint refused", wrong.status !== 0, wrong.stdout);
  const trusted = cli(["exec", "box", "--trust", info.fingerprint, "uname"]);
  check("cli: exact fingerprint accepted", trusted.status === 0 && trusted.stdout.includes("hello"), trusted.stderr);
  check("cli: pinned key needs no flag", cli(["exec", "box", "uname"]).status === 0);
  check("cli: exit code is reported", cli(["exec", "box", "fail"]).status !== 0);
  check("cli: git add is not destructive", cli(["exec", "box", "git", "add", "."]).status === 0);
  const destructive = cli(["exec", "box", "rm", "x"]);
  check("cli: destructive needs --yes", destructive.status !== 0 && destructive.stderr.includes("--yes"), destructive.stderr);
  check("cli: --yes runs it", cli(["exec", "box", "--yes", "rm", "x"]).status === 0);
  check("cli: password can be saved", cli(["password", "save", "box"]).status === 0);
  check("cli: saved password is used", cli(["exec", "box", "uname"], { password: false }).status === 0);
  const metrics = cli(["metrics", "box"]);
  check("cli: remote metrics", metrics.status === 0 && metrics.stdout.includes("ssh:/proc"), metrics.stderr);

  const local = join(work, "cli-files");
  mkdirSync(local, { recursive: true });
  writeFileSync(join(local, "up.txt"), "uploaded");
  check("cli: sftp put", cli(["sftp", "put", "box", join(local, "up.txt"), "/up.txt"]).status === 0);
  check("cli: sftp ls", cli(["sftp", "ls", "box", "--path", "/"]).stdout.includes("up.txt"));
  check("cli: sftp get", cli(["sftp", "get", "box", "/up.txt", join(local, "down.txt")]).status === 0 && readFileSync(join(local, "down.txt"), "utf8") === "uploaded");
  check("cli: sftp rm needs --yes", cli(["sftp", "rm", "box", "/up.txt"]).status !== 0);
  check("cli: sftp rm", cli(["sftp", "rm", "box", "/up.txt", "--yes"]).status === 0);
  const plan = cli(["sync", "plan", "box", "--local", local, "--remote", "/synced"]);
  check("cli: sync plan copes with a missing remote folder only by failing clearly", plan.status !== 0 || plan.stdout.includes("copy"), plan.stderr);
  mkdirSync(join(info.files, "synced"), { recursive: true });
  const apply = cli(["sync", "apply", "box", "--local", local, "--remote", "/synced"]);
  check("cli: sync apply", apply.status === 0 && existsSync(join(info.files, "synced", "up.txt")), apply.stderr);
  check("cli: host forget", cli(["host", "forget", "box"]).stdout.includes("forgotten"));
  check("cli: forgotten host is unknown again", cli(["exec", "box", "uname"]).status !== 0);
}

// --- the real window

const app = spawn(bin("termix-desktop.exe"), {
  env: {
    ...process.env,
    TERMIX_DATA_DIR: join(work, "app"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${debugPort}`,
  },
  stdio: "inherit",
});

let socket: WebSocket | undefined;
let nextId = 0;
const pending = new Map<number, (message: any) => void>();

async function connectToWindow() {
  const page = await until("the Termix window", async () => {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${debugPort}/json`)).json();
      return targets.find((item: { type: string }) => item.type === "page");
    } catch {
      return null;
    }
  }, 60000);
  const opened = new WebSocket(page.webSocketDebuggerUrl);
  socket = opened;
  await new Promise((done, fail) => {
    opened.onopen = done;
    opened.onerror = fail;
  });
  opened.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)?.(message);
      pending.delete(message.id);
    }
  };
}

function send(method: string, params: Record<string, unknown> = {}): Promise<any> {
  const id = ++nextId;
  socket?.send(JSON.stringify({ id, method, params }));
  return new Promise((done) => pending.set(id, done));
}

async function run(expression: string): Promise<any> {
  const { result } = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error(`${expression}: ${result.exceptionDetails.exception?.description}`);
  return result.result.value;
}

async function screenshot(name: string): Promise<void> {
  if (!shots) return;
  const { result } = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(shots, `${name}.png`), Buffer.from(result.data, "base64"));
}

async function runWindowTests() {
  await connectToWindow();
  await until("the app to load", () => run(`document.readyState === "complete" && Boolean(window.__TAURI_INTERNALS__)`));
  await send("Runtime.enable");
  await windowTests({ info, work, check, until, sleep, send, run, screenshot });
}

let crashed: unknown = null;
try {
  cliTests();
  await runWindowTests();
} catch (error) {
  crashed = error;
  console.error(error);
  await screenshot("failure").catch(() => {});
} finally {
  socket?.close();
  app.kill();
  server.kill();
  await sleep(500);
  rmSync(work, { recursive: true, force: true, maxRetries: 5 });
}
console.log(`\n${passed} passed, ${failures.length} failed${crashed ? ", stopped early" : ""}`);
process.exit(failures.length || crashed ? 1 : 0);
