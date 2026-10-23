import { invoke } from "@tauri-apps/api/core";
import { marked } from "marked";
import DOMPurify from "dompurify";
import hljs from "highlight.js";
import "highlight.js/styles/github-dark.css";
import "./style.css";

// ---------- markdown setup ----------
marked.setOptions({
  gfm: true,
  breaks: true,
});

// Highlight code blocks after markdown rendering.
const renderer = new marked.Renderer();
renderer.code = ({ text, lang }) => {
  const language = lang && hljs.getLanguage(lang) ? lang : "plaintext";
  let highlighted;
  try {
    highlighted = hljs.highlight(text, { language }).value;
  } catch {
    highlighted = DOMPurify.sanitize(text);
  }
  const langLabel = lang || "text";
  return `<div class="code-block">
    <div class="code-header"><span>${langLabel}</span><button class="copy-btn" data-code="${encodeURIComponent(text)}">Copy</button></div>
    <pre><code class="hljs language-${language}">${highlighted}</code></pre>
  </div>`;
};
marked.use({ renderer });

function renderMarkdown(text) {
  const raw = marked.parse(text);
  return DOMPurify.sanitize(raw, { ADD_ATTR: ["data-code"] });
}

// ---------- app state ----------
const app = document.getElementById("app");
let streamingMessage = null;
let streamingText = "";

app.innerHTML = `
  <div class="layout">
    <aside class="sidebar">
      <div class="brand"><span class="brand-dot"></span> Luna</div>
      <div class="sidebar-section">Session</div>
      <div class="session-item active">New chat</div>
      <div class="sidebar-footer">
        <div id="status" class="status idle">idle</div>
      </div>
    </aside>
    <main class="chat">
      <div id="messages" class="messages">
        <div class="welcome">
          <h1>Luna</h1>
          <p>AI agent harness — ask anything, or ask it to work on your code.</p>
        </div>
      </div>
      <div id="approval" class="approval hidden"></div>
      <form id="composer" class="composer">
        <textarea id="input" placeholder="Message Luna…  (Enter to send, Shift+Enter for newline)" rows="1"></textarea>
        <button id="send" type="submit" title="Send">↑</button>
        <button id="cancel" type="button" title="Cancel run" class="hidden">✕</button>
      </form>
    </main>
  </div>`;

const messagesEl = document.getElementById("messages");
const inputEl = document.getElementById("input");
const sendBtn = document.getElementById("send");
const cancelBtn = document.getElementById("cancel");
const statusEl = document.getElementById("status");
const approvalEl = document.getElementById("approval");

function scrollBottom() {
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function setStatus(s, cls) {
  statusEl.textContent = s;
  statusEl.className = "status " + cls;
}

function addUserMessage(text) {
  const div = document.createElement("div");
  div.className = "msg user";
  div.innerHTML = `<div class="msg-role">You</div><div class="msg-body">${DOMPurify.sanitize(text)}</div>`;
  messagesEl.appendChild(div);
  scrollBottom();
}

function ensureStreamingMessage() {
  if (!streamingMessage) {
    const div = document.createElement("div");
    div.className = "msg assistant streaming";
    div.innerHTML = `<div class="msg-role">Luna</div><div class="msg-body md"></div>`;
    messagesEl.appendChild(div);
    streamingMessage = div;
    streamingText = "";
  }
  return streamingMessage.querySelector(".msg-body");
}

function finishStreaming() {
  if (streamingMessage) {
    streamingMessage.classList.remove("streaming");
    streamingMessage = null;
    streamingText = "";
  }
}

function addToolCall(name, args, outcome, isError) {
  const div = document.createElement("div");
  div.className = "msg tool" + (isError ? " tool-error" : "");
  const argsStr = JSON.stringify(args, null, 2);
  div.innerHTML = `<div class="msg-role">tool · ${DOMPurify.sanitize(name)}</div>
    <div class="msg-body"><pre class="tool-args">${DOMPurify.sanitize(argsStr)}</pre>
    ${outcome ? `<pre class="tool-outcome">${DOMPurify.sanitize(outcome)}</pre>` : ""}</div>`;
  messagesEl.appendChild(div);
  scrollBottom();
}

// Copy buttons (event delegation).
document.addEventListener("click", (e) => {
  const btn = e.target.closest(".copy-btn");
  if (btn) {
    const code = decodeURIComponent(btn.dataset.code || "");
    navigator.clipboard.writeText(code);
    btn.textContent = "Copied!";
    setTimeout(() => (btn.textContent = "Copy"), 1200);
  }
});

// ---------- event stream ----------
async function listen() {
  const { listen } = await import("@tauri-apps/api/event");
  await listen("luna://event", (event) => handleEvent(event.payload));
  // Also listen on per-agent channels for multi-agent runs.
  await listen("luna://event/*", (event) => handleEvent(event.payload));
}

function handleEvent(ev) {
  switch (ev.type) {
    case "token_delta": {
      const body = ensureStreamingMessage();
      streamingText += ev.text;
      body.innerHTML = renderMarkdown(streamingText);
      scrollBottom();
      break;
    }
    case "assistant_message":
      finishStreaming();
      break;
    case "tool_call_requested":
      addToolCall(ev.name, ev.arguments, null, false);
      break;
    case "tool_call_finished":
      addToolCall(ev.name, null, ev.outcome, ev.is_error);
      break;
    case "approval_requested":
      showApproval(ev);
      break;
    case "approval_resolved":
      hideApproval();
      break;
    case "run_started":
      setStatus("running", "running");
      cancelBtn.classList.remove("hidden");
      sendBtn.disabled = true;
      break;
    case "run_finished":
      finishStreaming();
      setStatus("idle", "idle");
      cancelBtn.classList.add("hidden");
      sendBtn.disabled = false;
      if (ev.error) addSystemError(ev.error);
      break;
    case "orchestrator_transition":
      addTransition(ev.description);
      break;
    case "agent_spawned":
      addTransition(`spawned agent: ${ev.agent_id}${ev.parent ? ` (child of ${ev.parent})` : ""}`);
      break;
  }
}

function addSystemError(text) {
  const div = document.createElement("div");
  div.className = "msg system-error";
  div.textContent = text;
  messagesEl.appendChild(div);
  scrollBottom();
}

function addTransition(text) {
  const div = document.createElement("div");
  div.className = "transition";
  div.textContent = "⟡ " + text;
  messagesEl.appendChild(div);
  scrollBottom();
}

function showApproval(ev) {
  approvalEl.classList.remove("hidden");
  approvalEl.innerHTML = `
    <div class="approval-title">Approval required</div>
    <div class="approval-tool">${DOMPurify.sanitize(ev.tool_name)}</div>
    <pre class="approval-args">${DOMPurify.sanitize(JSON.stringify(ev.arguments, null, 2))}</pre>
    <div class="approval-actions">
      <button id="approve-yes" class="primary">Approve</button>
      <button id="approve-no">Deny</button>
    </div>`;
  document.getElementById("approve-yes").onclick = () => respond(ev.request_id, true);
  document.getElementById("approve-no").onclick = () => respond(ev.request_id, false);
  scrollBottom();
}

function hideApproval() {
  approvalEl.classList.add("hidden");
  approvalEl.innerHTML = "";
}

async function respond(requestId, approved) {
  await invoke("luna_respond_approval", { requestId, approved });
  hideApproval();
}

// ---------- composer ----------
inputEl.addEventListener("input", () => {
  inputEl.style.height = "auto";
  inputEl.style.height = Math.min(inputEl.scrollHeight, 200) + "px";
});

document.getElementById("composer").addEventListener("submit", async (e) => {
  e.preventDefault();
  const text = inputEl.value.trim();
  if (!text) return;
  inputEl.value = "";
  inputEl.style.height = "auto";
  addUserMessage(text);
  setStatus("running", "running");
  cancelBtn.classList.remove("hidden");
  sendBtn.disabled = true;
  try {
    await invoke("luna_run_agent", { text });
  } catch (err) {
    addSystemError(String(err));
  } finally {
    finishStreaming();
    setStatus("idle", "idle");
    cancelBtn.classList.add("hidden");
    sendBtn.disabled = false;
  }
});

cancelBtn.addEventListener("click", () => invoke("luna_cancel"));

listen();
