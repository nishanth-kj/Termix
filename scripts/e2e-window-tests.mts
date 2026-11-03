// The steps run inside the real Termix window. See e2e-windows.mjs.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface Harness {
  info: { port: string; fingerprint: string; password: string; files: string };
  work: string;
  check: (name: string, condition: unknown, detail?: string) => void;
  until: <T>(what: string, read: () => T | Promise<T>, timeout?: number) => Promise<NonNullable<T>>;
  sleep: (ms: number) => Promise<void>;
  send: (method: string, params?: Record<string, unknown>) => Promise<any>;
  run: (expression: string) => Promise<any>;
  screenshot: (name: string) => Promise<void>;
}

export async function windowTests({ info, work, check, until, sleep, send, run, screenshot }: Harness): Promise<void> {
  const text = (selector: string): Promise<string> => run(`document.querySelector(${JSON.stringify(selector)}).textContent`);
  const click = (selector: string) => run(`document.querySelector(${JSON.stringify(selector)}).click()`);
  const tab = (name: string) => click(`.tab[data-tab="${name}"]`);
  const submit = (selector: string) => run(`document.querySelector(${JSON.stringify(selector)}).requestSubmit()`);
  const isOpen = (selector: string): Promise<boolean> => run(`document.querySelector(${JSON.stringify(selector)}).open`);

  async function setField(selector: string, value: string) {
    await run(`(() => {
      const field = document.querySelector(${JSON.stringify(selector)});
      field.value = ${JSON.stringify(value)};
      field.dispatchEvent(new Event("input", { bubbles: true }));
      field.dispatchEvent(new Event("change", { bubbles: true }));
    })()`);
  }

  async function answer(accept: boolean, input?: string) {
    await until("a question", () => isOpen("#ask"));
    const body = `${await text("#ask-title")} ${await text("#ask-code")}`;
    if (input !== undefined) await setField("#ask-input", input);
    await click(accept ? "#ask-ok" : '#ask button[value="cancel"]');
    await until("the question to close", async () => !(await isOpen("#ask")));
    return body;
  }

  async function signIn(password: string, { remember = false } = {}) {
    await until("the sign-in dialog", () => isOpen("#signin-dialog"));
    await setField('#signin-form [name="password"]', password);
    if (remember) await run(`document.querySelector('#signin-form [name="remember"]').checked = true`);
    await submit("#signin-form");
    await until("sign-in to close", async () => !(await isOpen("#signin-dialog")));
  }

  // Native pickers cannot be automated, so the test answers for them.
  const answerPicker = (name: string, value: string) => run(`termixE2E.pickers.${name} = async () => ${JSON.stringify(value)}`);

  const runCommand = async (command: string) => {
    await setField('#run-form [name="command"]', command);
    await submit("#run-form");
  };
  const lastRun = (): Promise<string> => run(`[...document.querySelectorAll("#transcript .run")].at(-1)?.textContent ?? ""`);
  const lastRunIs = (fragment: string) => until(`a run with ${fragment}`, async () => (await lastRun()).includes(fragment));

  // First launch.
  await until("the sidebar", async () => (await text("#machine-list")).includes("None saved"));
  check("app: first launch welcomes the person", (await text("#welcome")).includes("first machine"));
  await screenshot("welcome");

  // Adding a machine. The old form threw after saving, so this guards that regression.
  await click("#welcome-add");
  await until("the add dialog", () => isOpen("#add-dialog"));
  await setField('#add-form [name="name"]', "box");
  await setField('#add-form [name="host"]', "127.0.0.1");
  await setField('#add-form [name="port"]', info.port);
  await setField('#add-form [name="username"]', "ops");
  await screenshot("add-machine");
  await submit("#add-form");
  await until("the machine view", async () => (await text("#machine-title")) === "box");
  check("app: saving closes the dialog and opens the machine", !(await isOpen("#add-dialog")));
  check("app: the sidebar lists it", (await text("#machine-list")).includes("ops@127.0.0.1"));
  check("app: a new host is marked unverified", (await text("#machine-state")) === "Not verified yet");
  await screenshot("overview-new");

  // Sign-in, then the host key, both only when needed.
  await tab("commands");
  await runCommand("uname -a");
  await until("the sign-in dialog", () => isOpen("#signin-dialog"));
  await screenshot("sign-in");
  await click("#signin-cancel");
  await lastRunIs("cancelled");
  check("app: sign-in can be cancelled", true);

  await runCommand("uname -a");
  await signIn("wrong-password");
  await until("the trust question", () => isOpen("#ask"));
  await screenshot("trust-dialog");
  const trustBody = await answer(false);
  check("app: the fingerprint is shown before trusting", trustBody.includes(info.fingerprint), trustBody);
  await lastRunIs("cancelled");
  check("app: declining the key runs nothing", true);

  await runCommand("uname -a");
  await signIn("wrong-password");
  await answer(true);
  await until("sign-in to ask again", () => isOpen("#signin-dialog"));
  check("app: a wrong password asks again and says why", (await text("#signin-error")).includes("did not work"));
  await signIn(info.password);
  await lastRunIs("exit 0");
  check("app: command output and exit code", (await lastRun()).includes("hello"), await lastRun());
  await until("the verified state", async () => (await text("#machine-state")) === "Host key verified");
  check("app: the key is pinned once the login worked", true);

  await runCommand("fail");
  await lastRunIs("exit 3");
  check("app: the password is reused in this session", !(await isOpen("#signin-dialog")));
  check("app: stderr and a failing exit code", (await lastRun()).includes("broken"));

  await runCommand("rm -rf /srv");
  const warning = await answer(false);
  check("app: a destructive command asks first", warning.includes("destructive") && warning.includes("rm -rf /srv"), warning);
  await lastRunIs("cancelled");
  await runCommand("rm -rf /srv");
  await answer(true);
  await lastRunIs("exit 0");
  await runCommand("rm -rf /again");
  check("app: consent does not carry over", (await answer(false)).includes("rm -rf /again"));
  await lastRunIs("cancelled");
  await runCommand("git add .");
  await lastRunIs("git add .");
  await lastRunIs("exit 0");
  check("app: git add runs without a warning", !(await isOpen("#ask")));
  await screenshot("commands");

  // Terminal over SSH, surviving a visit to another tab.
  const terminalText = () =>
    run(`(() => { const buffer = termixE2E.activeSession().term.buffer.active; let all = ""; for (let i = 0; i < buffer.length; i++) all += buffer.getLine(i).translateToString(true) + "\\n"; return all; })()`);
  await tab("terminal");
  await screenshot("terminal-empty");
  await click("#term-connect");
  await until("the SSH terminal", () => run(`termixE2E.hasLiveShell(termixE2E.currentId())`));
  check("app: the terminal says where it is connected", (await text("#term-status")).includes("box"));
  await run(`termixE2E.invoke("shell_write", { shell: termixE2E.activeSession().shell, data: "ping-over-ssh" })`);
  await until("the SSH echo", async () => (await terminalText()).includes("ping-over-ssh"));
  check("app: SSH terminal round trip", true);
  await sleep(200);
  await screenshot("terminal-ssh");
  await tab("overview");
  check("app: the sidebar shows a live session", await run(`Boolean(document.querySelector("#machine-list .side-dot"))`));
  await tab("terminal");
  check("app: the terminal survives switching tabs", (await terminalText()).includes("ping-over-ssh") && (await run(`termixE2E.hasLiveShell(termixE2E.currentId())`)));
  await click("#term-close");
  await until("the disconnect", async () => (await text("#term-status")) === "Not connected");
  check("app: disconnecting offers to reconnect", (await text("#term-connect")) === "Reconnect");

  // A local shell on this device.
  await click("#this-device");
  await until("the local view", async () => (await text("#machine-title")) === "This device");
  await click("#term-connect");
  await until("the local shell", () => run(`termixE2E.hasLiveShell(termixE2E.currentId())`));
  await run(`termixE2E.invoke("shell_write", { shell: termixE2E.activeSession().shell, data: "echo local-marker\\r\\n" })`);
  await until("local shell output", async () => (await terminalText()).includes("local-marker"));
  check("app: local shell on Windows", true);
  await sleep(300);
  await screenshot("terminal-local");
  await click("#term-close");

  // This device: metrics.
  await tab("monitor");
  await until("local metrics", async () => (await text("#metrics")).includes("Largest processes"));
  check("app: local metrics include disks and processes", (await text("#metrics")).includes("Disks") && (await text("#metrics-source")).includes("operating system"));
  await screenshot("monitor");

  // Files.
  await run(`document.querySelector('[data-machine]').click()`);
  await tab("files");
  await until("the first listing", async () => (await text("#files-status")) !== "Loading…");
  await click("#files-go");
  await answer(true, "/");
  await until("the root listing", async () => (await run(`termixE2E.currentFolder()`)) === "/");
  await click("#files-mkdir");
  await answer(true, "from-app");
  await until("the new folder", async () => (await text("#file-list")).includes("from-app"));
  check("app: create a remote folder", existsSync(join(info.files, "from-app")));
  writeFileSync(join(work, "send.txt"), "file from the app");
  await answerPicker("openFile", join(work, "send.txt"));
  await click("#files-upload");
  await until("the upload", async () => (await text("#file-list")).includes("send.txt"));
  check("app: upload", readFileSync(join(info.files, "send.txt"), "utf8") === "file from the app");
  await screenshot("files");
  const rowAction = (file: string, label: string) =>
    run(`document.querySelector('.file-row[data-name=${JSON.stringify(file)}] button[aria-label^=${JSON.stringify(label)}]').click()`);
  await answerPicker("saveFile", join(work, "back.txt"));
  await rowAction("send.txt", "Download");
  await until("the download", async () => (await text("#files-status")).startsWith("Downloaded"));
  check("app: download", readFileSync(join(work, "back.txt"), "utf8") === "file from the app");
  await rowAction("send.txt", "Rename");
  await answer(true, "moved.txt");
  await until("the rename", async () => (await text("#file-list")).includes("moved.txt"));
  check("app: rename", existsSync(join(info.files, "moved.txt")));
  await rowAction("moved.txt", "Delete");
  await answer(false);
  check("app: delete can be declined", existsSync(join(info.files, "moved.txt")));
  await rowAction("moved.txt", "Delete");
  await answer(true);
  await until("the delete", async () => !(await text("#file-list")).includes("moved.txt"));
  check("app: delete", !existsSync(join(info.files, "moved.txt")));
  await run(`document.querySelector('.file-row[data-name="from-app"] .file-name').click()`);
  await until("opening the folder", async () => (await run(`termixE2E.currentFolder()`)) === "/from-app");
  check("app: breadcrumbs follow the folder", (await text("#crumbs")).includes("from-app"));
  await click("#files-up");
  await until("going up", async () => (await run(`termixE2E.currentFolder()`)) === "/");
  check("app: folder navigation", true);

  // Remote metrics.
  await tab("monitor");
  await until("remote metrics", async () => (await text("#metrics-source")).includes("/proc"));
  check("app: remote metrics", (await text("#metrics")).includes("1 h 1 m"), await text("#metrics"));

  // Sync.
  await tab("sync");
  const folder = join(work, "sync-folder");
  mkdirSync(join(folder, "nested"), { recursive: true });
  writeFileSync(join(folder, "nested", "file.txt"), "synced");
  mkdirSync(join(info.files, "site"), { recursive: true });
  writeFileSync(join(info.files, "site", "keep.txt"), "keep");
  await answerPicker("openFolder", folder);
  await click("#sync-pick");
  await until("the picked folder", async () => (await run(`document.querySelector('#sync-form [name="local"]').value`)) === folder);
  await setField('#sync-form [name="remote"]', "/site");
  await submit("#sync-form");
  await until("the plan", async () => (await text("#sync-output")).includes("Will copy"));
  check("app: the plan lists extras and copies nothing yet", (await text("#sync-output")).includes("keep.txt") && !existsSync(join(info.files, "site", "nested")));
  await screenshot("sync");
  await click("#sync-apply");
  await until("the copy", async () => (await text("#sync-output")).startsWith("Copied"));
  check("app: sync applied without deleting", readFileSync(join(info.files, "site", "nested", "file.txt"), "utf8") === "synced" && existsSync(join(info.files, "site", "keep.txt")));

  // Saving a password survives forgetting the session's copy.
  await run(`termixE2E.sessionCredentials.clear()`);
  await tab("commands");
  await runCommand("uname -a");
  await signIn(info.password, { remember: true });
  await lastRunIs("exit 0");
  await until("the saved state", () => run(`termixE2E.hasSavedPassword()`));
  await run(`termixE2E.sessionCredentials.clear()`);
  await runCommand("uname -r");
  await lastRunIs("uname -r");
  await lastRunIs("exit 0");
  check("app: a saved password signs in without asking", !(await isOpen("#signin-dialog")));
  const stored = readFileSync(join(work, "app", "termix.sqlite")).toString("latin1");
  check("app: the password is not readable in the database", !stored.includes(info.password));

  // Overview.
  await tab("overview");
  check("app: overview shows the pinned key", (await text("#overview-details")).includes(info.fingerprint));
  await click("#overview-probe");
  await until("the port test", async () => (await text("#overview-status")).includes("accepted"));
  await screenshot("overview");

  // What works.
  await click("#open-support");
  await until("the support list", async () => (await text("#support-list")).includes("Not implemented yet"));
  check("app: the support list", (await text("#support-list")).includes("Folder sync apply"));
  await screenshot("support");
  await run(`document.querySelector('[data-machine]').click()`);

  // Light theme.
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "light" }] });
  await tab("files");
  await until("the listing", async () => (await text("#file-list")).includes("from-app"));
  await screenshot("light-files");
  await tab("overview");
  await screenshot("light-overview");
  await send("Emulation.setEmulatedMedia", { features: [] });

  // A phone-sized window: a list screen, then a detail screen with tabs at the thumb.
  await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
  await run(`termixE2E.goToList()`);
  await sleep(250);
  const overflow = () => run(`document.documentElement.scrollWidth - document.documentElement.clientWidth`);
  check("app: phone list has no sideways scroll", (await overflow()) <= 0);
  await screenshot("phone-list");
  await run(`document.querySelector('[data-machine]').click()`);
  await sleep(250);
  check("app: choosing a machine opens its detail screen", await run(`document.body.dataset.view === "detail" && getComputedStyle(document.querySelector(".sidebar")).display === "none"`));
  await screenshot("phone-overview");
  for (const name of ["terminal", "files", "commands", "monitor"]) {
    await tab(name);
    await sleep(name === "monitor" ? 1500 : 500);
    check(`app: phone ${name} has no sideways scroll`, (await overflow()) <= 0);
    const bounds = await run(`(() => { const box = document.querySelector("#${name}").getBoundingClientRect(); return box.right <= innerWidth + 1 && box.bottom <= document.querySelector("#tabs").getBoundingClientRect().top + 1; })()`);
    check(`app: phone ${name} fits above the tabs`, bounds);
    await screenshot(`phone-${name}`);
  }
  await run(`document.querySelector(".machine-view .back").click()`);
  check("app: back returns to the list", await run(`document.body.dataset.view === "list"`));
  await send("Emulation.clearDeviceMetricsOverride");

  // Forgetting and deleting.
  await run(`document.querySelector('[data-machine]').click()`);
  await tab("overview");
  await click("#forget-password");
  await until("the password to go", () => run(`!termixE2E.hasSavedPassword()`));
  await click("#forget-host");
  await answer(true);
  await until("the key to go", async () => (await text("#machine-state")) === "Not verified yet");
  check("app: forgetting a key returns to unverified", true);
  await click("#delete-machine");
  await answer(true);
  await until("the machine to go", async () => (await text("#machine-list")).includes("None saved"));
  check("app: delete returns to the welcome screen", !(await run(`document.querySelector("#welcome").hidden`)));
}
