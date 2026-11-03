const output = document.querySelector("#output");

function invoke(command, args) {
  if (!window.__TAURI__ || !window.__TAURI__.core) {
    return Promise.reject(new Error("Open this screen inside the Termix app. It is not a website."));
  }
  return window.__TAURI__.core.invoke(command, args);
}

function show(error) {
  output.textContent = error instanceof Error ? error.message : String(error);
}

document.querySelectorAll("button.nav").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll("button.nav").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    for (const section of document.querySelectorAll("main section")) {
      section.hidden = section.id !== button.dataset.tab;
    }
    if (button.dataset.tab === "monitor") loadMetrics();
    if (button.dataset.tab === "support") loadSupport();
  });
});

function fillMachineSelect(machines, selectedId) {
  const select = document.querySelector("#run-form [name=id]");
  const previous = selectedId ?? select.value;
  select.replaceChildren();
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.disabled = true;
  placeholder.textContent = machines.length === 0 ? "Save a host first" : "Select a saved host";
  select.append(placeholder);
  for (const machine of machines) {
    const option = document.createElement("option");
    option.value = machine.id;
    option.textContent = `${machine.name} (${machine.username}@${machine.host}:${machine.port})`;
    select.append(option);
  }
  if ([...select.options].some((option) => option.value === previous)) {
    select.value = previous;
  } else {
    placeholder.selected = true;
  }
}

function selectMachine(id) {
  const select = document.querySelector("#run-form [name=id]");
  if ([...select.options].some((option) => option.value === id)) {
    select.value = id;
  }
}

async function loadMachines(selectedId) {
  const list = document.querySelector("#machine-list");
  try {
    const machines = await invoke("list_machines");
    fillMachineSelect(machines, selectedId);
    list.replaceChildren();
    if (machines.length === 0) {
      const empty = document.createElement("p");
      empty.className = "mute";
      empty.textContent = "No machines saved yet.";
      list.append(empty);
      return;
    }
    for (const machine of machines) {
      const card = document.createElement("button");
      card.type = "button";
      card.className = "card pick";
      const title = document.createElement("strong");
      title.textContent = machine.name;
      const meta = document.createElement("p");
      meta.className = "mute";
      meta.textContent = `${machine.username}@${machine.host}:${machine.port} · ${machine.id}`;
      card.append(title, meta);
      card.addEventListener("click", () => selectMachine(machine.id));
      list.append(card);
    }
  } catch (error) {
    show(error);
  }
}

document.querySelector("#add-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget);
  const status = document.querySelector("#add-status");
  try {
    const machine = await invoke("add_machine", {
      name: data.get("name"),
      host: data.get("host"),
      username: data.get("username"),
      port: Number(data.get("port")) || 22,
    });
    status.textContent = `Saved ${machine.id}`;
    event.currentTarget.reset();
    event.currentTarget.port.value = "22";
    await loadMachines(machine.id);
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
  }
});

document.querySelector("#probe").addEventListener("click", async () => {
  const data = new FormData(document.querySelector("#run-form"));
  if (!data.get("id")) {
    show(new Error("Select a saved host first."));
    return;
  }
  try {
    output.textContent = await invoke("probe_machine", { id: data.get("id") });
  } catch (error) {
    show(error);
  }
});

document.querySelector("#run-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(event.currentTarget);
  try {
    const result = await invoke("run_command", {
      id: data.get("id"),
      command: data.get("command"),
      password: data.get("password"),
      trust: data.get("trust") === "on",
      yes: data.get("yes") === "on",
    });
    output.textContent = `${result.stdout}${result.stderr}\nexit ${result.exit_code ?? "unknown"}`;
    event.currentTarget.password.value = "";
  } catch (error) {
    show(error);
  }
});

async function loadMetrics() {
  const box = document.querySelector("#metrics");
  try {
    const sample = await invoke("local_metrics");
    const cpu = sample.cpu_percent == null ? "no delta yet" : `${sample.cpu_percent.toFixed(1)}%`;
    const used = sample.memory_used_bytes == null ? "unknown" : `${(sample.memory_used_bytes / 1024 ** 3).toFixed(1)} GiB`;
    const total = sample.memory_total_bytes == null ? "unknown" : `${(sample.memory_total_bytes / 1024 ** 3).toFixed(1)} GiB`;
    box.textContent = `CPU ${cpu}\nMemory ${used} / ${total}\nSource ${sample.source}`;
  } catch (error) {
    box.textContent = error instanceof Error ? error.message : String(error);
  }
}

async function loadSupport() {
  const list = document.querySelector("#support-list");
  try {
    const rows = await invoke("support");
    list.replaceChildren();
    for (const row of rows) {
      const card = document.createElement("div");
      card.className = "card";
      const name = document.createElement("span");
      name.textContent = row.name;
      const state = document.createElement("span");
      state.className = row.implemented ? "ok" : "bad";
      state.textContent = row.implemented ? "  implemented" : "  not implemented";
      card.append(name, state);
      list.append(card);
    }
  } catch (error) {
    show(error);
  }
}

loadMachines();
