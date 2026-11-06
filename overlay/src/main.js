// Hermechan overlay front-end. No bundler, no npm: Tauri exposes globals via
// `withGlobalTauri: true`. We listen for "from-plugin" events (state / say /
// action / point / config) and send chat lines back through the
// `send_to_plugin` command.
const { event, core } = window.__TAURI__ || {};

const body = document.body;
const bubble = document.getElementById("bubble");
const bubbleText = document.getElementById("bubble-text");
const stateTag = document.getElementById("state-tag");
const input = document.getElementById("chatinput");
const form = document.getElementById("chatbar");

let bubbleTimer = null;

function setState(state) {
  if (!state) return;
  body.dataset.state = state;
  stateTag.textContent = state;
}

function showBubble(text, emotion) {
  if (!text) return;
  body.dataset.emotion = emotion || "neutral";
  bubbleText.textContent = text;
  bubble.classList.remove("hidden");
  if (bubbleTimer) clearTimeout(bubbleTimer);
  // keep the bubble up a bit longer for longer lines
  const ms = Math.min(9000, 2500 + text.length * 55);
  bubbleTimer = setTimeout(() => bubble.classList.add("hidden"), ms);
}

function flashAction(verb) {
  // brief emphasis when she performs a desktop action
  setState(verb === "type" || verb === "key" ? "working" : "working");
}

function handle(msg) {
  switch (msg.type) {
    case "state":
      setState(msg.state);
      break;
    case "say":
      setState("speaking");
      showBubble(msg.text, msg.emotion);
      break;
    case "emote":
      body.dataset.emotion = msg.emotion || "neutral";
      break;
    case "point":
      setState("peek");
      break;
    case "action":
      flashAction(msg.verb);
      break;
    case "config":
      // reserved: mischief toggle etc.
      break;
    default:
      break;
  }
}

if (event && event.listen) {
  event.listen("from-plugin", (e) => {
    try {
      handle(typeof e.payload === "string" ? JSON.parse(e.payload) : e.payload);
    } catch (_) {
      /* ignore malformed */
    }
  });
}

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const text = input.value.trim();
  if (!text) return;
  input.value = "";
  setState("listening");
  if (core && core.invoke) {
    core.invoke("send_to_plugin", { msg: JSON.stringify({ type: "chat", text }) }).catch(() => {});
  }
});

// initial
setState("idle");
