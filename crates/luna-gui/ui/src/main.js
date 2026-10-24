import { invoke } from "@tauri-apps/api/core";
import { marked } from "marked";
import DOMPurify from "dompurify";
import hljs from "highlight.js";
import "highlight.js/styles/github-dark.css";
import "./style.css";

// ---------- markdown ----------
marked.setOptions({ gfm: true, breaks: true });

const renderer = new marked.Renderer();
renderer.code = ({ text, lang }) => {
  const language = lang && hljs.getLanguage(lang) ? lang : "plaintext";
  let highlighted;
  try {
    highlighted = hljs.highlight(text, { language }).value;
  } catch {
    highlighted = DOMPurify.sanitize(text);
  }
  return `<div class="code-block">
    <div class="code-header"><span>${lang || "text"}</span><button class="copy-btn" data-code="${encodeURIComponent(text)}">Copy</button></div>
    <pre><code class="hljs language-${language}">${highlighted}</code></pre>
  </div>`;
};
marked.use({ renderer });

function md(text) {
  return DOMPurify.sanitize(marked.parse(text), { ADD_ATTR: ["data-code"] });
}

// ---------- state ----------
const state = {
  items: [], // {kind:'user'|'assistant'|'tool'|'note'|'error', ...}
  agents: new Map(),
  running: false,
  model: "",
  tokens: 0,
  sessions: [],
  activeSession: null,
  slashIndex: 0,
  slashOpen: false,
  settingsOpen: false,
};

const SLASH = [
  { name: "/new", desc: "start a new session" },
  { name: "/sessions", desc: "browse saved sessions" },
  { name: "/model", desc: "show active provider and model" },
  { name: "/clear", desc: "clear the transcript" },
  { name: "/settings", desc: "open provider settings" },
  { name: "/help", desc: "show commands and shortcuts" },
];

// ---------- shell ----------
const app = document.getElementById("app");
app.innerHTML = `
  <div class="layout">
    <aside class="sidebar">
      <div class="brand"><span class="brand-dot"></span> Luna</div>
      <button id="new-chat" class="new-chat">+ New chat</button>
      <input id="session-search" class="session-search" placeholder="Search sessions…" />
      <div id="session-list" class="session-list"></div>
      <div class="sidebar-footer">
        <button id="settings-btn" class="settings-btn">⚙ Settings</button>
      </div>
    </aside>
    <main class="main">
      <header class="topbar">
        <div id="topbar-title" class="topbar-title">New chat</div>
        <div id="topbar-model" class="topbar-model"></div>
      </header>
      <div id="transcript" class="transcript">
        <div class="welcome">
          <h1>Luna</h1>
          <p>Ask anything, or ask it to work on your code.</p>
          <div class="welcome-hints">
            <span><kbd>/</kbd> commands</span>
            <span><kbd>Ctrl</kbd>+<kbd>K</kbd> search</span>
            <span><kbd>Esc</kbd> stop</span>
          </div>
        </div>
      </div>
      <div id="approval" class="approval hidden"></div>
      <div class="composer-wrap">
        <div id="slash-menu" class="slash-menu hidden"></div>
        <form id="composer" class="composer">
          <textarea id="input" rows="1" placeholder="Message Luna…"></textarea>
          <div class="composer-actions">
            <button id="send" type="submit" class="send-btn" title="Send">↑</button>
            <button id="stop" type="button" class="stop-btn hidden" title="Stop">■</button>
          </div>
        </form>
      </div>
    </main>
    <aside id="right-panel" class="right-panel hidden"></aside>
  </div>
  <div id="settings-overlay" class="overlay hidden"></div>
  <div id="sessions-overlay" class="overlay hidden"></div>`;

const $ = (id) => document.getElementById(id);
const transcriptEl = $("transcript");
const inputEl = $("input");
const slashEl = $("slash-menu");

// ---------- rendering ----------
function scrollBottom() {
  transcriptEl.scrollTop = transcriptEl.scrollHeight;
}

function renderTranscript() {
  if (state.items.length === 0) return;
  transcriptEl.innerHTML = "";
  for (const item of state.items) {
    transcriptEl.appendChild(renderItem(item));
  }
  scrollBottom();
}

function renderItem(item) {
  const el = document.createElement("div");
  switch (item.kind) {
    case "user":
      el.className = "msg user";
      el.innerHTML = `<div class="msg-role">You</div><div class="msg-body">${DOMPurify.sanitize(item.text)}</div>`;
      break;
    case "assistant":
      el.className = "msg assistant";
      el.innerHTML = `<div class="msg-role">Luna</div><div class="msg-body md">${md(item.text)}</div>`;
      break;
    case "tool":
      el.className = "tool-card" + (item.isError ? " err" : "");
      el.innerHTML = `
        <button class="tool-head">
          <span class="chev${item.expanded ? " open" : ""}">›</span>
          <span class="tool-name">${DOMPurify.sanitize(item.name)}</span>
          <span class="tool-args">${DOMPurify.sanitize(firstLine(item.args))}</span>
          <span class="tool-status">${item.running ? '<span class="spinner"></span>' : item.isError ? "✕" : "✓"}</span>
        </button>
        <div class="tool-body${item.expanded ? "" : " hidden"}">
          <pre class="tool-args-full">${DOMPurify.sanitize(item.args)}</pre>
          ${item.output ? `<pre class="tool-output">${DOMPurify.sanitize(item.output)}</pre>` : ""}
        </div>`;
      el.querySelector(".tool-head").onclick = () => {
        item.expanded = !item.expanded;
        renderTranscript();
      };
      break;
    case "note":
      el.className = "note";
      el.textContent = "⟡ " + item.text;
      break;
    case "error":
      el.className = "msg system-error";
      el.textContent = item.text;
      break;
  }
  return el;
}

function firstLine(s) {
  const l = (s || "").split("\n")[0];
  return l.length > 70 ? l.slice(0, 69) + "…" : l;
}

function pushItem(item) {
  state.items.push(item);
  transcriptEl.appendChild(renderItem(item));
  scrollBottom();
}

// Streaming assistant message: update in place.
let streamingEl = null;
let streamingText = "";
function streamDelta(text) {
  if (!streamingEl) {
    const last = state.items[state.items.length - 1];
    if (!last || last.kind !== "assistant" || last.done) {
      state.items.push({ kind: "assistant", text: "" });
    }
    streamingEl = renderItem(state.items[state.items.length - 1]);
    streamingEl.classList.add("streaming");
    transcriptEl.appendChild(streamingEl);
    streamingText = "";
  }
  streamingText += text;
  streamingEl.querySelector(".msg-body").innerHTML = md(streamingText);
  scrollBottom();
}

function finishStreaming() {
  if (streamingEl) {
    streamingEl.classList.remove("streaming");
    const last = state.items[state.items.length - 1];
    if (last && last.kind === "assistant") {
      last.text = streamingText;
      last.done = true;
    }
    streamingEl = null;
    streamingText = "";
  }
}

// ---------- sessions ----------
function renderSessions() {
  const list = $("session-list");
  const q = $("session-search").value.trim().toLowerCase();
  const filtered = state.sessions.filter(
    (s) => !q || (s.first_user_message || "").toLowerCase().includes(q)
  );
  if (filtered.length === 0) {
    list.innerHTML = `<div class="session-empty">${state.sessions.length ? "no matches" : "no sessions yet"}</div>`;
    return;
  }
  list.innerHTML = "";
  for (const s of filtered) {
    const el = document.createElement("div");
    el.className = "session-item" + (s.id === state.activeSession ? " active" : "");
    el.innerHTML = `<div class="session-title">${DOMPurify.sanitize(s.first_user_message || "Untitled")}</div>
      <div class="session-meta">${s.event_count} events</div>`;
    el.onclick = () => loadSession(s.id);
    list.appendChild(el);
  }
}

async function refreshSessions() {
  try {
    state.sessions = await invoke("luna_list_sessions", { limit: 50 });
    renderSessions();
  } catch {
    /* ignore */
  }
}

async function loadSession(id) {
  try {
    const events = await invoke("luna_load_session", { sessionId: id });
    state.items = [];
    state.activeSession = id;
    for (const ev of events) applyEvent(ev, true);
    renderTranscript();
    renderSessions();
    $("topbar-title").textContent = "Resumed session";
  } catch (err) {
    pushItem({ kind: "error", text: String(err) });
  }
}

// ---------- events ----------
function applyEvent(ev, replay = false) {
  const agentId = ev.agent_id || null;
  switch (ev.type) {
    case "token_delta":
      if (agentId && state.agents.has(agentId)) {
        state.agents.get(agentId).lastOutput += ev.text;
        renderAgents();
      }
      state.tokens += Math.round(ev.text.length / 4);
      streamDelta(ev.text);
      break;
    case "assistant_message":
      finishStreaming();
      break;
    case "tool_call_requested":
      if (agentId && state.agents.has(agentId)) {
        state.agents.get(agentId).toolCalls++;
        state.agents.get(agentId).status = `tool: ${ev.name}`;
        renderAgents();
      }
      pushItem({
        kind: "tool",
        name: ev.name,
        args: JSON.stringify(ev.arguments, null, 2),
        output: null,
        running: true,
        isError: false,
        expanded: false,
      });
      break;
    case "tool_call_finished": {
      const t = [...state.items].reverse().find((i) => i.kind === "tool" && i.running);
      if (t) {
        t.output = ev.outcome;
        t.isError = ev.is_error;
        t.running = false;
        renderTranscript();
      }
      break;
    }
    case "approval_requested":
      showApproval(ev);
      break;
    case "approval_resolved":
      hideApproval();
      break;
    case "run_started":
      setRunning(true);
      break;
    case "agent_spawned":
      state.agents.set(ev.agent_id, {
        parent: ev.parent || null,
        status: "spawned",
        lastOutput: "",
        toolCalls: 0,
      });
      renderAgents();
      pushItem({ kind: "note", text: `spawned ${ev.agent_id}${ev.parent ? ` (child of ${ev.parent})` : ""}` });
      break;
    case "orchestrator_transition":
      pushItem({ kind: "note", text: ev.description });
      break;
    case "run_finished":
      if (agentId && state.agents.has(agentId)) {
        const a = state.agents.get(agentId);
        if (ev.summary) a.lastOutput = ev.summary;
        a.status = ev.error ? "failed" : "done";
        renderAgents();
      }
      finishStreaming();
      setRunning(false);
      if (ev.error && ev.error !== "cancelled") pushItem({ kind: "error", text: ev.error });
      if (!replay) refreshSessions();
      break;
  }
}

function setRunning(running) {
  state.running = running;
  $("send").classList.toggle("hidden", running);
  $("stop").classList.toggle("hidden", !running);
  $("topbar-title").classList.toggle("busy", running);
}

// ---------- agents panel ----------
function renderAgents() {
  const panel = $("right-panel");
  if (state.agents.size === 0) {
    panel.classList.add("hidden");
    return;
  }
  panel.classList.remove("hidden");
  const rows = [...state.agents.entries()]
    .map(([id, a]) => {
      const icon = a.status === "done" ? "✓" : a.status === "failed" ? "✕" : a.status.startsWith("tool:") ? "⚙" : "●";
      const cls = a.status === "done" ? "ok" : a.status === "failed" ? "err" : "run";
      return `<div class="agent-row">
        <span class="agent-icon ${cls}">${icon}</span>
        <span class="agent-name">${DOMPurify.sanitize(id)}</span>
        <span class="agent-tools">${a.toolCalls}</span>
      </div>`;
    })
    .join("");
  panel.innerHTML = `<div class="panel-title">Agents</div>${rows}`;
}

// ---------- approval ----------
function showApproval(ev) {
  const el = $("approval");
  el.classList.remove("hidden");
  el.innerHTML = `
    <div class="approval-title">Approval required</div>
    <div class="approval-tool">${DOMPurify.sanitize(ev.tool_name)}</div>
    <pre class="approval-args">${DOMPurify.sanitize(JSON.stringify(ev.arguments, null, 2))}</pre>
    <div class="approval-actions">
      <button id="approve-yes" class="primary-btn">Approve</button>
      <button id="approve-no">Deny</button>
    </div>`;
  $("approve-yes").onclick = () => respond(ev.request_id, true);
  $("approve-no").onclick = () => respond(ev.request_id, false);
}

function hideApproval() {
  $("approval").classList.add("hidden");
  $("approval").innerHTML = "";
}

async function respond(requestId, approved) {
  await invoke("luna_respond_approval", { requestId, approved });
  hideApproval();
}

// ---------- slash menu ----------
function updateSlashMenu() {
  const v = inputEl.value;
  if (!v.startsWith("/") || v.includes(" ")) {
    slashEl.classList.add("hidden");
    state.slashOpen = false;
    return;
  }
  const matches = SLASH.filter((c) => c.name.startsWith(v.toLowerCase()));
  if (matches.length === 0) {
    slashEl.classList.add("hidden");
    state.slashOpen = false;
    return;
  }
  state.slashOpen = true;
  state.slashIndex = Math.min(state.slashIndex, matches.length - 1);
  slashEl.classList.remove("hidden");
  slashEl.innerHTML = matches
    .map(
      (c, i) =>
        `<div class="slash-item${i === state.slashIndex ? " on" : ""}" data-cmd="${c.name}">
          <span class="slash-name">${c.name}</span><span class="slash-desc">${c.desc}</span>
        </div>`
    )
    .join("");
  slashEl.querySelectorAll(".slash-item").forEach((el) => {
    el.onclick = () => {
      inputEl.value = el.dataset.cmd;
      updateSlashMenu();
      inputEl.focus();
    };
  });
}

// ---------- commands ----------
async function runCommand(cmd) {
  switch (cmd) {
    case "/new":
      state.items = [];
      state.activeSession = null;
      transcriptEl.innerHTML = "";
      $("topbar-title").textContent = "New chat";
      break;
    case "/clear":
      state.items = [];
      transcriptEl.innerHTML = "";
      break;
    case "/model":
      pushItem({ kind: "note", text: `active: ${state.model || "unknown"}` });
      break;
    case "/sessions":
      await refreshSessions();
      openSessionsOverlay();
      break;
    case "/settings":
      openSettings();
      break;
    case "/help":
      pushItem({
        kind: "note",
        text: "commands: /new /sessions /model /clear /settings /help · Enter send · Esc stop · Ctrl+K search",
      });
      break;
    default:
      pushItem({ kind: "error", text: `unknown command: ${cmd}` });
  }
}

// ---------- composer ----------
inputEl.addEventListener("input", () => {
  inputEl.style.height = "auto";
  inputEl.style.height = Math.min(inputEl.scrollHeight, 220) + "px";
  updateSlashMenu();
});

inputEl.addEventListener("keydown", (e) => {
  if (state.slashOpen) {
    const matches = SLASH.filter((c) => c.name.startsWith(inputEl.value.toLowerCase()));
    if (e.key === "ArrowDown") {
      e.preventDefault();
      state.slashIndex = Math.min(state.slashIndex + 1, matches.length - 1);
      updateSlashMenu();
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      state.slashIndex = Math.max(state.slashIndex - 1, 0);
      updateSlashMenu();
      return;
    }
    if (e.key === "Tab" || (e.key === "Enter" && matches.length > 0 && inputEl.value !== matches[0].name)) {
      e.preventDefault();
      inputEl.value = matches[state.slashIndex].name;
      updateSlashMenu();
      return;
    }
  }
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    $("composer").requestSubmit();
  }
  if (e.key === "Escape" && state.running) {
    invoke("luna_cancel");
  }
});

$("composer").addEventListener("submit", async (e) => {
  e.preventDefault();
  const text = inputEl.value.trim();
  if (!text || state.running) return;
  inputEl.value = "";
  inputEl.style.height = "auto";
  slashEl.classList.add("hidden");
  state.slashOpen = false;

  if (text.startsWith("/")) {
    await runCommand(text.split(" ")[0]);
    return;
  }

  pushItem({ kind: "user", text });
  setRunning(true);
  try {
    await invoke("luna_run_agent", { text });
  } catch (err) {
    pushItem({ kind: "error", text: String(err) });
  } finally {
    finishStreaming();
    setRunning(false);
  }
});

$("stop").onclick = () => invoke("luna_cancel");
$("new-chat").onclick = () => runCommand("/new");
$("session-search").addEventListener("input", renderSessions);

// ---------- settings ----------
const PROVIDER_LABELS = {
  anthropic: "Anthropic (Claude)",
  openai: "OpenAI",
  google: "Google Gemini",
  xai: "xAI (Grok)",
  deepseek: "DeepSeek",
  mistral: "Mistral",
  openrouter: "OpenRouter",
  groq: "Groq",
};

async function openSettings() {
  const overlay = $("settings-overlay");
  overlay.classList.remove("hidden");
  overlay.innerHTML = `<div class="dialog"><div class="dialog-loading">Loading…</div></div>`;
  try {
    const [providers, active] = await Promise.all([
      invoke("luna_list_providers"),
      invoke("luna_active_provider"),
    ]);
    renderSettings(providers, active);
  } catch (err) {
    overlay.innerHTML = `<div class="dialog"><div class="system-error">${DOMPurify.sanitize(String(err))}</div></div>`;
  }
}

function renderSettings(providers, active) {
  const overlay = $("settings-overlay");
  const rows = providers
    .map(
      (p) => `
    <div class="provider-row" data-key="${DOMPurify.sanitize(p.key)}">
      <div class="provider-head">
        <label class="switch"><input type="checkbox" class="p-enabled" ${p.enabled ? "checked" : ""}/><span class="slider"></span></label>
        <strong>${DOMPurify.sanitize(PROVIDER_LABELS[p.key] || p.key)}</strong>
        ${active === p.key ? `<span class="badge">default</span>` : ""}
        <button class="p-default link-btn" data-key="${DOMPurify.sanitize(p.key)}">set default</button>
      </div>
      <div class="provider-fields">
        <input class="p-model" placeholder="model" value="${DOMPurify.sanitize(p.model || "")}"/>
        <input class="p-key" type="password" placeholder="API key${p.has_api_key ? " (saved)" : ""}"/>
        <button class="p-save primary-btn" data-key="${DOMPurify.sanitize(p.key)}">Save</button>
      </div>
    </div>`
    )
    .join("");

  overlay.innerHTML = `
    <div class="dialog">
      <div class="dialog-head"><strong>Providers</strong><button id="settings-close" class="link-btn">close</button></div>
      <div class="dialog-note">Keys are stored in ~/.luna/config.toml. Env vars are used when no key is set here.</div>
      ${rows}
      <div class="dialog-head sub"><strong>Custom OpenAI-compatible provider</strong></div>
      <div class="provider-fields">
        <input id="c-key" placeholder="name"/>
        <input id="c-url" placeholder="base URL (…/v1)"/>
        <input id="c-model" placeholder="model"/>
        <input id="c-api" type="password" placeholder="API key (optional)"/>
        <button id="c-save" class="primary-btn">Add</button>
      </div>
    </div>`;

  $("settings-close").onclick = () => overlay.classList.add("hidden");
  overlay.onclick = (e) => {
    if (e.target === overlay) overlay.classList.add("hidden");
  };

  overlay.querySelectorAll(".p-save").forEach((btn) => {
    btn.onclick = async () => {
      const key = btn.dataset.key;
      const row = overlay.querySelector(`.provider-row[data-key="${key}"]`);
      try {
        await invoke("luna_save_provider", {
          key,
          enabled: row.querySelector(".p-enabled").checked,
          model: row.querySelector(".p-model").value.trim(),
          apiKey: row.querySelector(".p-key").value.trim(),
          baseUrl: "",
        });
        btn.textContent = "Saved ✓";
        setTimeout(() => (btn.textContent = "Save"), 1200);
      } catch (err) {
        btn.textContent = String(err).slice(0, 30);
      }
    };
  });

  overlay.querySelectorAll(".p-default").forEach((btn) => {
    btn.onclick = async () => {
      await invoke("luna_set_default_provider", { key: btn.dataset.key });
      openSettings();
    };
  });

  $("c-save").onclick = async () => {
    try {
      await invoke("luna_save_custom_provider", {
        key: $("c-key").value.trim(),
        baseUrl: $("c-url").value.trim(),
        model: $("c-model").value.trim(),
        apiKey: $("c-api").value.trim(),
      });
      openSettings();
    } catch (err) {
      alert(String(err));
    }
  };
}

$("settings-btn").onclick = openSettings;

// ---------- sessions overlay ----------
function openSessionsOverlay() {
  const overlay = $("sessions-overlay");
  overlay.classList.remove("hidden");
  const rows = state.sessions
    .map(
      (s) =>
        `<div class="session-row" data-id="${DOMPurify.sanitize(s.id)}">
          <div class="session-title">${DOMPurify.sanitize(s.first_user_message || "Untitled")}</div>
          <div class="session-meta">${s.event_count} events · ${new Date(s.last_activity).toLocaleString()}</div>
        </div>`
    )
    .join("");
  overlay.innerHTML = `<div class="dialog">
    <div class="dialog-head"><strong>Sessions</strong><button id="sessions-close" class="link-btn">close</button></div>
    ${rows || '<div class="dialog-note">no sessions yet</div>'}
  </div>`;
  $("sessions-close").onclick = () => overlay.classList.add("hidden");
  overlay.onclick = (e) => {
    if (e.target === overlay) overlay.classList.add("hidden");
  };
  overlay.querySelectorAll(".session-row").forEach((el) => {
    el.onclick = () => {
      loadSession(el.dataset.id);
      overlay.classList.add("hidden");
    };
  });
}

// ---------- global keys ----------
document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "k") {
    e.preventDefault();
    $("session-search").focus();
    $("session-search").select();
  }
  if (e.key === "Escape") {
    $("settings-overlay").classList.add("hidden");
    $("sessions-overlay").classList.add("hidden");
  }
});

// copy buttons
document.addEventListener("click", (e) => {
  const btn = e.target.closest(".copy-btn");
  if (btn) {
    navigator.clipboard.writeText(decodeURIComponent(btn.dataset.code || ""));
    btn.textContent = "Copied!";
    setTimeout(() => (btn.textContent = "Copy"), 1200);
  }
});

// ---------- boot ----------
async function boot() {
  const { listen } = await import("@tauri-apps/api/event");
  await listen("luna://event", (event) => applyEvent(event.payload));
  try {
    state.model = await invoke("luna_active_provider");
    $("topbar-model").textContent = state.model;
  } catch {
    /* ignore */
  }
  await refreshSessions();
}

boot();
