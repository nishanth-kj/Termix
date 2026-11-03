import "@xterm/xterm/css/xterm.css";
import "./app.css";

import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import {
  appErrorOf,
  invoke,
  messageOf,
  shellChannel,
  type CommandOutput,
  type Credentials,
  type Listing,
  type Machine,
  type RemoteEntry,
  type Sample,
  type SupportRow,
  type SyncPlan,
} from "./api";

const LOCAL = "local";
type Target = Machine | { id: typeof LOCAL; name: string };
type Tab = "overview" | "terminal" | "files" | "commands" | "monitor" | "sync";

const $ = <T extends HTMLElement = HTMLElement>(selector: string): T => {
  const node = document.querySelector<T>(selector);
  if (!node) throw new Error(`missing ${selector}`);
  return node;
};
const field = (form: HTMLFormElement, name: string) => form.elements.namedItem(name) as HTMLInputElement;

let machines: Machine[] = [];
let current: Target | null = null;
let currentTab: Tab = "overview";
const tabByMachine = new Map<string, Tab>();
const sessionCredentials = new Map<string, Credentials>();

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function icon(name: string): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "icon");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;
function toast(text: string): void {
  const box = $("#toast");
  box.textContent = text;
  box.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    box.hidden = true;
  }, 4000);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

/* Questions */

interface Question {
  title: string;
  body: string;
  confirm: string;
  code?: string | null;
  input?: string;
  value?: string;
}

/** Native alert and confirm are not available on every platform, so ask in the page. */
function ask(question: Question & { input: string }): Promise<string | null>;
function ask(question: Question): Promise<true | null>;
function ask({ title, body, code, confirm, input, value }: Question): Promise<string | true | null> {
  const dialog = $<HTMLDialogElement>("#ask");
  $("#ask-title").textContent = title;
  $("#ask-body").textContent = body;
  const codeBox = $("#ask-code");
  codeBox.hidden = !code;
  codeBox.textContent = code ?? "";
  const inputBox = $<HTMLInputElement>("#ask-input");
  $("#ask-input-label").hidden = !input;
  $("#ask-input-name").textContent = input ?? "";
  inputBox.value = value ?? "";
  $("#ask-ok").textContent = confirm;
  dialog.returnValue = "cancel";
  dialog.showModal();
  return new Promise((resolve) => {
    const onClose = () => {
      // The close event is queued, so one from the previous question can arrive
      // after this dialog has opened again.
      if (dialog.open) return;
      dialog.removeEventListener("close", onClose);
      if (dialog.returnValue !== "ok") resolve(null);
      else resolve(input ? inputBox.value : true);
    };
    dialog.addEventListener("close", onClose);
  });
}

/** Native file pickers on desktop. iOS and Android do not hand out plain paths. */
const pickers = {
  saveFile: (name: string): Promise<string | null> => saveDialog({ defaultPath: name }),
  openFile: (): Promise<string | null> => openDialog({ multiple: false, directory: false }),
  openFolder: (): Promise<string | null> => openDialog({ multiple: false, directory: true }),
};

function askCredentials(machine: Machine, failed: boolean): Promise<Credentials | null> {
  const dialog = $<HTMLDialogElement>("#signin-dialog");
  const form = $<HTMLFormElement>("#signin-form");
  const cancel = $("#signin-cancel");
  form.reset();
  $("#signin-title").textContent = `Sign in to ${machine.name}`;
  $("#signin-body").textContent = `${machine.username}@${machine.host}:${machine.port}`;
  $("#signin-error").textContent = failed ? "That did not work. Check the password or key and try again." : "";
  dialog.showModal();
  field(form, "password").focus();
  return new Promise((resolve) => {
    const finish = (value: Credentials | null) => {
      form.removeEventListener("submit", onSubmit);
      cancel.removeEventListener("click", onCancel);
      dialog.removeEventListener("cancel", onCancel);
      dialog.close();
      resolve(value);
    };
    const onCancel = () => finish(null);
    const onSubmit = (event: SubmitEvent) => {
      event.preventDefault();
      const credentials: Credentials = {
        password: field(form, "password").value || null,
        keyPath: field(form, "keyPath").value || null,
        keyPassphrase: field(form, "keyPassphrase").value || null,
        remember: field(form, "remember").checked,
      };
      if (!credentials.password && !credentials.keyPath) {
        $("#signin-error").textContent = "Enter a password or choose a private key.";
        return;
      }
      finish(credentials);
    };
    form.addEventListener("submit", onSubmit);
    cancel.addEventListener("click", onCancel);
    dialog.addEventListener("cancel", onCancel);
  });
}

function currentMachine(): Machine {
  if (!current || current.id === LOCAL) throw new Error("Select a saved machine first.");
  return current as Machine;
}

interface Consent {
  id: string;
  credentials: Credentials;
  trust: string | null;
  confirmed: boolean;
}

/**
 * Runs something that connects to the current machine and handles whatever the
 * person has to decide: sign-in, a new host key, or a destructive command.
 * A new host key is shown for comparison, and only that exact fingerprint is
 * sent back to be pinned. Consent lasts for this one attempt. Returns undefined
 * if the person declines.
 */
async function withConsent<T>(action: (consent: Consent) => Promise<T>, command?: string): Promise<T | undefined> {
  const machine = currentMachine();
  let trust: string | null = null;
  let confirmed = false;
  const stored = sessionCredentials.get(machine.id);
  let credentials: Credentials = stored ? { ...stored, remember: false } : {};
  for (;;) {
    try {
      const result = await action({ id: machine.id, credentials, trust, confirmed });
      if (credentials.password || credentials.keyPath) {
        sessionCredentials.set(machine.id, { ...credentials, remember: false });
      }
      if (credentials.remember || !machine.pinned_fingerprint) await loadMachines();
      return result;
    } catch (error) {
      const failure = appErrorOf(error);
      if (failure?.kind === "needs_credentials") {
        sessionCredentials.delete(machine.id);
        const entered = await askCredentials(machine, failure.failed);
        if (!entered) return undefined;
        credentials = entered;
      } else if (failure?.kind === "host_key_unknown" && !trust) {
        const accepted = await ask({
          title: "Check this host key",
          body: `${machine.name} is new to Termix. Compare this ${failure.algorithm} fingerprint with the server (ssh-keygen -lf /etc/ssh/ssh_host_*.pub). Your password has not been sent.`,
          code: failure.fingerprint,
          confirm: "It matches, trust it",
        });
        if (!accepted) return undefined;
        trust = failure.fingerprint;
      } else if (failure?.kind === "needs_confirmation" && !confirmed) {
        const accepted = await ask({
          title: "This command looks destructive",
          body: "It can delete data or interrupt the machine. This check is a heuristic, so read the command again.",
          code: command,
          confirm: "Run it",
        });
        if (!accepted) return undefined;
        confirmed = true;
      } else if (failure?.kind === "host_key_changed") {
        throw new Error(`${failure.message}. Someone may be intercepting this connection. If the server was really reinstalled, forget its host key on the Overview tab.`);
      } else {
        throw error;
      }
    }
  }
}

/* Navigation */

const isLocal = (): boolean => current?.id === LOCAL;
const LOCAL_TARGET: Target = { id: LOCAL, name: "This device" };

function showOnly(view: "#welcome" | "#support" | "#machine-view"): void {
  for (const id of ["#welcome", "#support", "#machine-view"]) $(id).hidden = id !== view;
  document.body.dataset.view = "detail";
}

function goToList(): void {
  document.body.dataset.view = "list";
}

document.querySelectorAll(".back").forEach((button) => button.addEventListener("click", goToList));

function renderSidebar(): void {
  const list = $("#machine-list");
  list.replaceChildren(
    ...machines.map((machine) => {
      const item = element("button", `side-item${current?.id === machine.id ? " active" : ""}`);
      item.type = "button";
      item.dataset.machine = machine.id;
      const badge = element("span", "side-icon");
      badge.append(icon("server"));
      if (hasLiveShell(machine.id)) badge.append(element("span", "side-dot"));
      const text = element("span", "side-text");
      text.append(element("strong", "", machine.name), element("small", "", `${machine.username}@${machine.host}`));
      item.append(badge, text);
      item.addEventListener("click", () => selectMachine(machine.id));
      return item;
    }),
  );
  if (machines.length === 0) list.append(element("p", "side-empty", "None saved yet."));
  $("#this-device").classList.toggle("active", isLocal());
}

function selectMachine(id: string): void {
  current = id === LOCAL ? LOCAL_TARGET : (machines.find((machine) => machine.id === id) ?? null);
  if (!current) return;
  renderSidebar();
  renderHeader();
  showOnly("#machine-view");
  const local = id === LOCAL;
  document.querySelectorAll<HTMLElement>(".tab").forEach((button) => {
    button.hidden = local && !["terminal", "monitor"].includes(button.dataset.tab ?? "");
  });
  // The monitor is the one local tool every platform has.
  const fallback: Tab = local ? (document.body.classList.contains("mobile") ? "monitor" : "terminal") : "overview";
  showTab(tabByMachine.get(id) ?? fallback);
}

function renderHeader(): void {
  const state = $("#machine-state");
  if (!current || current.id === LOCAL) {
    $("#machine-title").textContent = "This device";
    $("#machine-subtitle").textContent = "No connection needed";
    state.hidden = true;
    return;
  }
  const machine = currentMachine();
  $("#machine-title").textContent = machine.name;
  $("#machine-subtitle").textContent = `${machine.username}@${machine.host}:${machine.port}`;
  state.hidden = false;
  state.textContent = machine.pinned_fingerprint ? "Host key verified" : "Not verified yet";
  state.className = `pill ${machine.pinned_fingerprint ? "ok" : "warn"}`;
}

function showTab(tab: Tab): void {
  const button = $(`.tab[data-tab="${tab}"]`);
  if (button.hidden || !current) return;
  currentTab = tab;
  tabByMachine.set(current.id, tab);
  document.querySelectorAll(".tab").forEach((item) => {
    const active = item === button;
    item.classList.toggle("active", active);
    item.setAttribute("aria-selected", String(active));
  });
  for (const section of document.querySelectorAll<HTMLElement>("#machine-view > section")) {
    section.hidden = section.id !== tab;
  }
  if (tab === "overview") renderOverview();
  if (tab === "terminal") showTerminal();
  if (tab === "files") openFilesTab();
  if (tab === "commands") showTranscript();
  if (tab === "monitor") void refreshMetrics();
}

document.querySelectorAll<HTMLElement>(".tab").forEach((button) => {
  button.addEventListener("click", () => showTab(button.dataset.tab as Tab));
});
$("#this-device").addEventListener("click", () => selectMachine(LOCAL));
$("#open-support").addEventListener("click", () => {
  current = null;
  renderSidebar();
  showOnly("#support");
  void loadSupport();
});

/* Machines */

async function loadMachines(): Promise<void> {
  try {
    machines = await invoke<Machine[]>("list_machines");
  } catch (error) {
    $("#machine-list").replaceChildren(element("p", "error", messageOf(error)));
    return;
  }
  if (current && current.id !== LOCAL) {
    const id = current.id;
    current = machines.find((machine) => machine.id === id) ?? null;
    if (current) {
      renderHeader();
      if (currentTab === "overview" && !$("#machine-view").hidden) renderOverview();
    }
  }
  renderSidebar();
}

function openAddDialog(): void {
  $<HTMLFormElement>("#add-form").reset();
  $("#add-status").textContent = "";
  $<HTMLDialogElement>("#add-dialog").showModal();
}
$("#add-machine").addEventListener("click", openAddDialog);
$("#welcome-add").addEventListener("click", openAddDialog);
$("#add-cancel").addEventListener("click", () => $<HTMLDialogElement>("#add-dialog").close());

$<HTMLFormElement>("#add-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = $<HTMLFormElement>("#add-form");
  try {
    const machine = await invoke<Machine>("add_machine", {
      name: field(form, "name").value,
      host: field(form, "host").value,
      username: field(form, "username").value,
      port: Number(field(form, "port").value) || 22,
    });
    $<HTMLDialogElement>("#add-dialog").close();
    await loadMachines();
    selectMachine(machine.id);
    toast(`Saved ${machine.name}`);
  } catch (error) {
    $("#add-status").textContent = messageOf(error);
  }
});

function renderOverview(): void {
  const machine = currentMachine();
  const rows: [string, string, string?][] = [
    ["Address", `${machine.host}:${machine.port}`, "mono"],
    ["Username", machine.username, "mono"],
    machine.pinned_fingerprint
      ? ["Host key", machine.pinned_fingerprint, "mono"]
      : ["Host key", "Not verified yet. You will be asked to compare it the first time you connect.", "unverified"],
    ["Password", machine.has_saved_password ? "Saved, encrypted on this device" : "Not saved"],
    ["Last connected", machine.last_connected_at ? new Date(machine.last_connected_at).toLocaleString() : "Never"],
  ];
  $("#overview-details").replaceChildren(
    ...rows.flatMap(([label, value, className]) => [element("dt", "", label), element("dd", className ?? "", value)]),
  );
  $<HTMLButtonElement>("#forget-host").disabled = !machine.pinned_fingerprint;
  $<HTMLButtonElement>("#forget-password").disabled = !machine.has_saved_password;
  $("#overview-status").textContent = "";
}

$("#overview-terminal").addEventListener("click", () => showTab("terminal"));
$("#overview-files").addEventListener("click", () => showTab("files"));
$("#overview-probe").addEventListener("click", async () => {
  const status = $("#overview-status");
  status.textContent = "Testing…";
  try {
    const result = await invoke<string>("probe_machine", { id: currentMachine().id });
    status.textContent = `The port accepted a connection (${result}). That does not prove the machine is healthy.`;
  } catch (error) {
    status.textContent = messageOf(error);
  }
});
$("#forget-host").addEventListener("click", async () => {
  const machine = currentMachine();
  const yes = await ask({
    title: "Forget this host key?",
    body: "Do this only if the server's key really changed. You will be asked to verify the new one.",
    code: machine.pinned_fingerprint,
    confirm: "Forget it",
  });
  if (!yes) return;
  await invoke("forget_host", { id: machine.id });
  await loadMachines();
});
$("#forget-password").addEventListener("click", async () => {
  const { id } = currentMachine();
  await invoke("forget_password", { id });
  sessionCredentials.delete(id);
  await loadMachines();
  toast("Saved password removed");
});
$("#delete-machine").addEventListener("click", async () => {
  const machine = currentMachine();
  const yes = await ask({
    title: `Delete ${machine.name}?`,
    body: "This removes the saved host and its saved password from this device. Nothing changes on the machine.",
    confirm: "Delete",
  });
  if (!yes) return;
  await closeSession(machine.id);
  await invoke("delete_machine", { id: machine.id });
  sessionCredentials.delete(machine.id);
  current = null;
  await loadMachines();
  showOnly("#welcome");
  goToList();
  toast(`Deleted ${machine.name}`);
});

/* Terminal: each machine keeps its own terminal while you move around the app. */

interface TerminalSession {
  id: string;
  host: HTMLElement;
  term: Terminal;
  fit: FitAddon;
  shell: number | null;
  status: string;
  failed: boolean;
  everConnected: boolean;
}

const terminals = new Map<string, TerminalSession>();

function hasLiveShell(id: string): boolean {
  return Boolean(terminals.get(id)?.shell);
}

function terminalFor(id: string): TerminalSession {
  const existing = terminals.get(id);
  if (existing) return existing;
  const host = element("div", "term-session");
  $("#term-host").append(host);
  const term = new Terminal({
    cursorBlink: true,
    fontFamily: 'ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace',
    fontSize: 13,
    scrollback: 5000,
    theme: { background: "#0e1014", foreground: "#ece9e2", cursor: "#39ff14", selectionBackground: "#39ff1455" },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host);
  const session: TerminalSession = { id, host, term, fit, shell: null, status: "", failed: false, everConnected: false };
  term.onData((data) => {
    if (session.shell) invoke("shell_write", { shell: session.shell, data }).catch(() => {});
  });
  term.onResize(({ cols, rows }) => {
    if (session.shell) invoke("shell_resize", { shell: session.shell, columns: cols, rows }).catch(() => {});
  });
  terminals.set(id, session);
  return session;
}

function activeSession(): TerminalSession | undefined {
  return current ? terminals.get(current.id) : undefined;
}

function renderTerminalState(): void {
  if (!current) return;
  const session = activeSession();
  const live = Boolean(session?.shell);
  const local = isLocal();
  const status = $("#term-status");
  status.textContent = live ? (local ? "Local shell" : `Connected to ${current.name}`) : "Not connected";
  status.classList.toggle("live", live);
  $("#term-close").hidden = !live;
  $("#term-empty").hidden = live;
  const text = $("#term-empty-text");
  text.classList.toggle("failed", Boolean(session?.failed));
  text.textContent = session?.status || (local ? "Open a shell on this device." : `Open an interactive shell on ${current.name}.`);
  $("#term-connect").textContent = session?.everConnected ? "Reconnect" : local ? "Open shell" : "Connect";
}

function showTerminal(): void {
  if (!current) return;
  const session = terminalFor(current.id);
  for (const other of terminals.values()) other.host.hidden = other !== session;
  renderTerminalState();
  session.fit.fit();
  if (session.shell) session.term.focus();
}

new ResizeObserver(() => {
  const session = activeSession();
  if (session && !$("#terminal").hidden) session.fit.fit();
}).observe($("#term-host"));

function channelFor(session: TerminalSession) {
  return shellChannel((event) => {
    if (event.kind === "output") session.term.write(new Uint8Array(event.data));
    else markClosed(session, "The session ended.");
  });
}

function markClosed(session: TerminalSession, status: string): void {
  if (session.shell) invoke("shell_close", { shell: session.shell }).catch(() => {});
  session.shell = null;
  session.status = status;
  session.failed = false;
  renderSidebar();
  if (current?.id === session.id && !$("#terminal").hidden) renderTerminalState();
}

async function closeSession(id: string): Promise<void> {
  const session = terminals.get(id);
  if (!session) return;
  if (session.shell) await invoke("shell_close", { shell: session.shell }).catch(() => {});
  session.term.dispose();
  session.host.remove();
  terminals.delete(id);
}

$<HTMLButtonElement>("#term-connect").addEventListener("click", async () => {
  if (!current) return;
  const session = terminalFor(current.id);
  const button = $<HTMLButtonElement>("#term-connect");
  button.disabled = true;
  session.status = isLocal() ? "Starting…" : `Connecting to ${current.name}…`;
  session.failed = false;
  renderTerminalState();
  try {
    session.term.reset();
    session.fit.fit();
    const { cols: columns, rows } = session.term;
    const shell = isLocal()
      ? await invoke<number>("open_local_shell", { columns, rows, output: channelFor(session) })
      : await withConsent(({ id, credentials, trust }) =>
          invoke<number>("open_ssh_shell", { id, columns, rows, credentials, trust, output: channelFor(session) }),
        );
    session.status = "";
    if (shell !== undefined) {
      session.shell = shell;
      session.everConnected = true;
      renderSidebar();
      session.term.focus();
    }
  } catch (error) {
    session.status = messageOf(error);
    session.failed = true;
  } finally {
    button.disabled = false;
    renderTerminalState();
  }
});

$("#term-close").addEventListener("click", () => {
  const session = activeSession();
  if (session) markClosed(session, "Disconnected.");
});

/* Files */

const filesStatus = $("#files-status");
const filePaths = new Map<string, string>();

function currentFolder(): string {
  return (current && filePaths.get(current.id)) || ".";
}

function joinRemote(folder: string, name: string): string {
  return folder.endsWith("/") ? `${folder}${name}` : `${folder}/${name}`;
}

function parentOf(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const index = trimmed.lastIndexOf("/");
  return index <= 0 ? "/" : trimmed.slice(0, index);
}

function renderCrumbs(path: string): void {
  const parts = path.split("/").filter(Boolean);
  const link = (label: string, target: string) => {
    const button = element("button", "", label);
    button.type = "button";
    button.addEventListener("click", () => void listFiles(target));
    return button;
  };
  const items: HTMLElement[] = [link("/", "/")];
  parts.forEach((part, index) => {
    if (index > 0) items.push(element("span", "", "/"));
    items.push(link(part, `/${parts.slice(0, index + 1).join("/")}`));
  });
  $("#crumbs").replaceChildren(...items);
}

/** Runs a file action with consent and reports its failure in the status line. */
async function fileAction<T>(run: (consent: Consent) => Promise<T>, done?: (result: T) => void | Promise<void>): Promise<void> {
  try {
    const result = await withConsent(run);
    if (result === undefined) {
      filesStatus.textContent = "Cancelled.";
      return;
    }
    if (done) await done(result);
  } catch (error) {
    filesStatus.textContent = messageOf(error);
  }
}

function rowButton(iconName: string, label: string, handler: () => void, className = ""): HTMLButtonElement {
  const button = element("button", `icon-button ${className}`);
  button.type = "button";
  button.title = label;
  button.setAttribute("aria-label", label);
  button.append(icon(iconName));
  button.addEventListener("click", handler);
  return button;
}

function fileRow(entry: RemoteEntry): HTMLElement {
  const row = element("div", `file-row${entry.is_dir ? " dir" : ""}`);
  row.dataset.name = entry.name;
  row.append(icon(entry.is_dir ? "folder" : entry.is_symlink ? "link" : "file"));
  if (entry.is_dir) {
    const open = element("button", "file-name", entry.name);
    open.type = "button";
    open.addEventListener("click", () => void listFiles(entry.path));
    row.append(open);
  } else {
    row.append(element("span", "file-name", entry.name));
  }
  row.append(element("span", "file-meta size", entry.is_dir ? "" : formatBytes(entry.size)));
  row.append(element("span", "file-meta date", entry.modified_unix ? new Date(entry.modified_unix * 1000).toLocaleString() : ""));
  const actions = element("div", "file-actions");
  if (!entry.is_dir) {
    actions.append(
      rowButton(
        "download",
        `Download ${entry.name}`,
        async () => {
          const local = await pickers.saveFile(entry.name);
          if (!local) return;
          filesStatus.textContent = `Downloading ${entry.name}…`;
          await fileAction(
            ({ id, credentials, trust }) => invoke<number>("sftp_download", { id, credentials, trust, remote: entry.path, local }),
            (bytes) => {
              filesStatus.textContent = `Downloaded ${formatBytes(bytes)} to ${local}`;
            },
          );
        },
        "desktop-only",
      ),
    );
  }
  actions.append(
    rowButton("pencil", `Rename ${entry.name}`, async () => {
      const name = await ask({ title: `Rename ${entry.name}`, body: "Enter the new name.", input: "New name", value: entry.name, confirm: "Rename" });
      if (!name || name === entry.name) return;
      await fileAction(
        ({ id, credentials, trust }) => invoke("sftp_rename", { id, credentials, trust, from: entry.path, to: joinRemote(parentOf(entry.path), name) }),
        () => listFiles(currentFolder()),
      );
    }),
    rowButton(
      "trash",
      `Delete ${entry.name}`,
      async () => {
        const yes = await ask({
          title: `Delete ${entry.name}?`,
          body: entry.is_dir ? "Only an empty folder can be deleted. This cannot be undone." : "This cannot be undone.",
          code: entry.path,
          confirm: "Delete",
        });
        if (!yes) return;
        await fileAction(
          ({ id, credentials, trust }) => invoke("sftp_remove", { id, credentials, trust, path: entry.path, isDir: entry.is_dir }),
          () => listFiles(currentFolder()),
        );
      },
      "bad-hover",
    ),
  );
  row.append(actions);
  return row;
}

async function listFiles(path: string): Promise<void> {
  filesStatus.textContent = "Loading…";
  const machineId = currentMachine().id;
  await fileAction(
    ({ id, credentials, trust }) => invoke<Listing>("sftp_list", { id, credentials, trust, path }),
    (listing) => {
      filePaths.set(machineId, listing.path);
      if (current?.id !== machineId) return;
      renderCrumbs(listing.path);
      filesStatus.textContent = listing.entries.length === 0 ? "This folder is empty." : "";
      const sorted = [...listing.entries].sort((a, b) => Number(b.is_dir) - Number(a.is_dir) || a.name.localeCompare(b.name));
      $("#file-list").replaceChildren(...sorted.map(fileRow));
    },
  );
}

function openFilesTab(): void {
  $("#file-list").replaceChildren();
  $("#crumbs").replaceChildren();
  void listFiles(currentFolder());
}

$("#files-up").addEventListener("click", () => void listFiles(parentOf(currentFolder())));
$("#files-refresh").addEventListener("click", () => void listFiles(currentFolder()));
$("#files-go").addEventListener("click", async () => {
  const path = await ask({ title: "Go to a folder", body: "Enter a path on the machine.", input: "Folder", value: currentFolder(), confirm: "Open" });
  if (path) await listFiles(path);
});
$("#files-mkdir").addEventListener("click", async () => {
  const name = await ask({ title: "New folder", body: `Create it inside ${currentFolder()}.`, input: "Folder name", confirm: "Create" });
  if (!name) return;
  await fileAction(
    ({ id, credentials, trust }) => invoke("sftp_mkdir", { id, credentials, trust, path: joinRemote(currentFolder(), name) }),
    () => listFiles(currentFolder()),
  );
});
$("#files-upload").addEventListener("click", async () => {
  const local = await pickers.openFile();
  if (!local) return;
  const name = local.split(/[\\/]/).pop() ?? local;
  filesStatus.textContent = `Uploading ${name}…`;
  await fileAction(
    ({ id, credentials, trust }) => invoke("sftp_upload", { id, credentials, trust, local, remote: joinRemote(currentFolder(), name) }),
    () => listFiles(currentFolder()),
  );
});

/* Commands */

const transcripts = new Map<string, HTMLElement[]>();
const transcriptHint = $("#transcript-empty");

function showTranscript(): void {
  if (!current) return;
  const box = $("#transcript");
  const runs = transcripts.get(current.id) ?? [];
  box.replaceChildren(...runs);
  if (runs.length === 0) box.append(transcriptHint);
  box.scrollTop = box.scrollHeight;
  field($<HTMLFormElement>("#run-form"), "command").focus();
}

$<HTMLFormElement>("#run-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const input = field($<HTMLFormElement>("#run-form"), "command");
  const command = input.value.trim();
  if (!command) return;
  const machineId = currentMachine().id;

  const run = element("article", "run");
  const head = element("div", "run-head");
  const exit = element("span", "exit", "running");
  head.append(element("code", "", command), exit);
  const out = element("pre", "", "");
  const err = element("pre", "stderr", "");
  run.append(head, out, err);
  transcripts.set(machineId, [...(transcripts.get(machineId) ?? []), run]);
  showTranscript();
  input.value = "";

  const finish = (label: string, className: string) => {
    exit.textContent = label;
    exit.className = `exit ${className}`;
    if (current?.id === machineId && !$("#commands").hidden) showTranscript();
  };
  try {
    const result = await withConsent(
      ({ id, credentials, trust, confirmed }) => invoke<CommandOutput>("run_command", { id, credentials, trust, confirmed, command }),
      command,
    );
    if (result === undefined) {
      finish("cancelled", "");
      return;
    }
    out.textContent = result.stdout;
    err.textContent = result.stderr;
    const code = result.exit_code;
    finish(code === null ? "exit unknown" : `exit ${code}`, code === 0 ? "ok" : "bad");
  } catch (error) {
    err.textContent = messageOf(error);
    finish("failed", "bad");
  }
});

/* Monitor */

/** `warn` is for real capacity. A process bar is only relative to the largest one. */
function meter(fraction: number, warn = true): HTMLElement {
  const bar = element("div", "meter");
  const fill = element("i", warn && fraction >= 0.9 ? "high" : "");
  fill.style.width = `${Math.min(100, Math.max(0, fraction * 100)).toFixed(1)}%`;
  bar.append(fill);
  return bar;
}

function stat(label: string, value: string, fraction: number | null, note?: string): HTMLElement {
  const box = element("div", "stat");
  box.append(element("small", "", label), element("strong", "", value));
  if (fraction !== null) box.append(meter(fraction));
  if (note) box.append(element("span", "", note));
  return box;
}

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return days > 0 ? `${days} d ${hours} h` : `${hours} h ${minutes} m`;
}

function renderMetrics(sample: Sample): void {
  const used = sample.memory_used_bytes;
  const total = sample.memory_total_bytes;
  const memoryKnown = used !== null && total !== null && total > 0;
  const cpu = sample.cpu_percent;
  const stats = element("div", "stats");
  stats.append(
    stat("CPU", cpu === null ? "—" : `${cpu.toFixed(0)}%`, cpu === null ? null : cpu / 100),
    stat(
      "Memory",
      memoryKnown ? `${((used / total) * 100).toFixed(0)}%` : "—",
      memoryKnown ? used / total : null,
      memoryKnown ? `${formatBytes(used)} of ${formatBytes(total)}` : undefined,
    ),
    stat("Uptime", sample.uptime_seconds === null ? "—" : formatUptime(sample.uptime_seconds), null),
  );
  const parts: HTMLElement[] = [stats];
  if (sample.disks.length > 0) {
    const group = element("div", "metric-group");
    group.append(element("h2", "", "Disks"));
    for (const disk of sample.disks) {
      const row = element("div", "metric-row");
      row.append(
        element("span", "mono", disk.mount),
        meter(disk.total_kib ? disk.used_kib / disk.total_kib : 0),
        element("span", "value", `${formatBytes(disk.used_kib * 1024)} of ${formatBytes(disk.total_kib * 1024)}`),
      );
      group.append(row);
    }
    parts.push(group);
  }
  if (sample.processes.length > 0) {
    const group = element("div", "metric-group");
    group.append(element("h2", "", "Largest processes by memory"));
    const largest = sample.processes[0].memory_bytes || 1;
    for (const process of sample.processes) {
      const row = element("div", "metric-row");
      row.append(element("span", "", process.name), meter(process.memory_bytes / largest, false), element("span", "value", formatBytes(process.memory_bytes)));
      group.append(row);
    }
    parts.push(group);
  }
  $("#metrics").replaceChildren(...parts);
}

async function refreshMetrics(): Promise<void> {
  const machineId = current?.id;
  const local = isLocal();
  const source = $("#metrics-source");
  source.textContent = "Reading…";
  try {
    const sample = local
      ? await invoke<Sample>("local_metrics")
      : await withConsent(({ id, credentials, trust }) => invoke<Sample>("machine_metrics", { id, credentials, trust }));
    if (current?.id !== machineId) return;
    if (sample === undefined) {
      source.textContent = "Cancelled.";
      return;
    }
    const origin = local ? "From this device's operating system" : "From /proc over SSH";
    source.textContent = `${origin}, ${new Date().toLocaleTimeString()}`;
    renderMetrics(sample);
  } catch (error) {
    source.textContent = "";
    $("#metrics").replaceChildren(element("p", "error", messageOf(error)));
  }
}

$("#metrics-refresh").addEventListener("click", () => void refreshMetrics());

/* Sync */

const syncForm = $<HTMLFormElement>("#sync-form");
const syncOutput = $("#sync-output");
const syncApply = $<HTMLButtonElement>("#sync-apply");
const syncDirection = $("#sync-direction");

function syncArguments() {
  return {
    local: field(syncForm, "local").value,
    remote: field(syncForm, "remote").value,
    pull: syncDirection.dataset.pull === "true",
    excludes: field(syncForm, "excludes")
      .value.split(",")
      .map((name) => name.trim())
      .filter(Boolean),
  };
}

function planGroup(title: string, note: string, lines: string[]): HTMLElement[] {
  if (lines.length === 0) return [];
  const group = element("div", "plan-group");
  const heading = element("h2", "", `${title} `);
  heading.append(element("small", "", `${lines.length} · ${note}`));
  group.append(heading, ...lines.map((line) => element("p", "", line)));
  return [group];
}

function renderPlan(plan: SyncPlan): void {
  const groups = [
    ...planGroup("Will copy", "newer or missing at the destination", plan.copies.map((copy) => copy.relative_path)),
    ...planGroup("Conflicts", "destination is newer, so these are skipped", plan.conflicts),
    ...planGroup("Only at the destination", "never deleted", plan.would_delete),
  ];
  syncOutput.replaceChildren(...(groups.length > 0 ? groups : [element("p", "hint", "Already in sync.")]));
}

function resetSyncPlan(): void {
  syncApply.disabled = true;
}

syncForm.addEventListener("input", resetSyncPlan);
syncDirection.addEventListener("click", () => {
  const pull = syncDirection.dataset.pull !== "true";
  syncDirection.dataset.pull = String(pull);
  $("#sync-direction-text").textContent = pull ? "Remote to local" : "Local to remote";
  resetSyncPlan();
});
$("#sync-pick").addEventListener("click", async () => {
  const folder = await pickers.openFolder();
  if (!folder) return;
  field(syncForm, "local").value = folder;
  resetSyncPlan();
});
$("#signin-pick").addEventListener("click", async () => {
  const path = await pickers.openFile();
  if (path) field($<HTMLFormElement>("#signin-form"), "keyPath").value = path;
});
syncForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  syncApply.disabled = true;
  syncOutput.replaceChildren(element("p", "hint", "Comparing folders…"));
  try {
    const args = syncArguments();
    const plan = await withConsent(({ id, credentials, trust }) => invoke<SyncPlan>("sync_plan", { id, credentials, trust, ...args }));
    if (plan === undefined) {
      syncOutput.replaceChildren(element("p", "hint", "Cancelled."));
      return;
    }
    renderPlan(plan);
    syncApply.disabled = plan.copies.length === 0;
  } catch (error) {
    syncOutput.replaceChildren(element("p", "error", messageOf(error)));
  }
});
syncApply.addEventListener("click", async () => {
  syncApply.disabled = true;
  syncOutput.replaceChildren(element("p", "hint", "Copying…"));
  try {
    const args = syncArguments();
    const copied = await withConsent(({ id, credentials, trust }) => invoke<number>("sync_apply", { id, credentials, trust, ...args }));
    syncOutput.replaceChildren(element("p", "hint", copied === undefined ? "Cancelled." : `Copied ${copied} file(s). Nothing was deleted.`));
  } catch (error) {
    syncOutput.replaceChildren(element("p", "error", messageOf(error)));
  }
});

/* What works */

async function loadSupport(): Promise<void> {
  const list = $("#support-list");
  try {
    const rows = await invoke<SupportRow[]>("support");
    const group = (title: string, on: boolean) => {
      const box = element("div", "support-group");
      box.append(element("h2", "", title));
      for (const row of rows.filter((item) => item.implemented === on)) {
        const line = element("div", `support-row${on ? " on" : ""}`);
        line.append(icon(on ? "check" : "minus"), element("span", "", row.name));
        box.append(line);
      }
      return box;
    };
    list.replaceChildren(group("Working now", true), group("Not implemented yet", false));
  } catch (error) {
    list.replaceChildren(element("p", "error", messageOf(error)));
  }
}

/* Start */

async function start(): Promise<void> {
  try {
    const { mobile } = await invoke<{ mobile: boolean }>("platform");
    document.body.classList.toggle("mobile", mobile);
  } catch {
    // Outside the app there is no backend; loadMachines reports that.
  }
  await loadMachines();
  document.body.dataset.view = "list";
  if (machines.length > 0 && window.matchMedia("(min-width: 721px)").matches) selectMachine(machines[0].id);
}

// Only an end-to-end build has this. Native pickers cannot be automated, so the
// tests answer for them, and they read state the page does not show as text.
if (import.meta.env.VITE_TERMIX_E2E) {
  Object.assign(window, {
    termixE2E: {
      pickers,
      invoke,
      activeSession,
      hasLiveShell,
      currentFolder,
      goToList,
      sessionCredentials,
      currentId: () => current?.id,
      hasSavedPassword: () => currentMachine().has_saved_password,
    },
  });
}

void start();
