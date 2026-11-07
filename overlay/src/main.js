// Hermes-chan overlay front-end. No bundler or npm dependencies.
const { event, core } = window.__TAURI__ || {};

const body = document.body;
const bubble = document.getElementById("bubble");
const bubbleText = document.getElementById("bubble-text");
const stateTag = document.getElementById("state-tag");
const statusMessage = document.getElementById("status-message");
const input = document.getElementById("chatinput");
const form = document.getElementById("chatbar");
const chara = document.getElementById("chara");
const followToggle = document.getElementById("follow-toggle");
const sprites = new window.HermesSpritePlayer(chara, "hermeschan/");

let manifest = null;
let bubbleTimer = null;
let currentState = "idle";
let currentEmotion = "neutral";
let activeAction = null;
let pendingAction = null;
let pendingStateMessage = null;
let transient = null;
let movementClip = null;
let sleeping = false;
let mischiefEnabled = false;
// Her own visual mischief runs until the Hermes plugin says otherwise (/hermeschan mischief off).
let mischiefConfigured = false;
let followActive = false;
let watching = false;
let latestActiveWindow = null;
let pendingChat = null;
let pendingChatTimer = null;
let hitRegionTimer = null;
let draggingPet = false;
let chatOpen = false;
let chatToggledAt = 0;
let wander = null;

function errorText(error) {
  if (typeof error === "string") return error;
  return error?.message || "unknown error";
}

function parsePayload(payload) {
  return typeof payload === "string" ? JSON.parse(payload) : payload;
}

function invokeNative(command, args) {
  if (!core?.invoke) return Promise.reject(new Error("desktop bridge is disconnected"));
  return core.invoke(command, args);
}

function sendToPlugin(payload) {
  return invokeNative("send_to_plugin", { msg: JSON.stringify(payload) });
}

// `copy`: a command this message tells you to run; clicking the message copies it.
// For her rest reminder: while Hermes works (or she acts for it) the card waits.
window.HermesChanBusy = () => Boolean(activeAction || pendingAction)
  || ["thinking", "working", "blocked", "listening"].includes(currentState);

// `settings`: the macOS Privacy & Security page that fixes it; clicking opens it.
function setStatus(text, kind = "", copy = "", settings = "") {
  const hint = copy ? "(click here to copy the command)"
    : settings ? "(click here: I'll open that page and show you what to drag in)" : "";
  statusMessage.textContent = text && hint ? `${text}\n${hint}` : text || "";
  statusMessage.dataset.kind = kind;
  statusMessage.dataset.copy = copy || "";
  statusMessage.dataset.settings = settings || "";
  statusMessage.classList.toggle("copyable", Boolean(text && hint));
  statusMessage.classList.toggle("hidden", !text);
  scheduleHitRegions();
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (_) {
    // Older webviews: copy through a hidden text field.
    const field = document.createElement("textarea");
    field.value = text;
    field.setAttribute("readonly", "");
    field.style.position = "fixed";
    field.style.opacity = "0";
    document.body.appendChild(field);
    field.select();
    let copied = false;
    try {
      copied = document.execCommand("copy");
    } catch (_) {
      copied = false;
    }
    field.remove();
    return copied;
  }
}

// Her window has no use for the browser's own right-click menu (just "Reload", which would
// reset her). The chat box keeps its Cut / Copy / Paste menu.
document.addEventListener("contextmenu", (e) => {
  if (e.target !== input) e.preventDefault();
});

statusMessage.addEventListener("click", async () => {
  const pane = statusMessage.dataset.settings;
  if (pane) {
    try {
      const program = await invokeNative("open_privacy_settings", { pane });
      setStatus(program
        ? `Drag ${program} from the Finder window into that list, then switch it on ✿`
        : "Switch on the app that runs Hermes in that list ✿", "warning");
    } catch (error) {
      setStatus(`Could not open System Settings: ${errorText(error)}`, "error");
    }
    return;
  }
  const command = statusMessage.dataset.copy;
  if (!command) return;
  const copied = await copyText(command);
  setStatus(copied ? "Copied! Paste it in a terminal and press Enter ✿"
    : `Copy this into a terminal: ${command}`, "warning");
  if (chatOpen) input.focus();
});

function setStateMetadata(state) {
  currentState = state;
  body.dataset.state = state;
  stateTag.textContent = state;
}

function effectiveState(requestedState) {
  const requested = requestedState || "idle";
  if (!manifest || manifest.states[requested]) return requested;
  return "idle";
}

function clipForState(state) {
  if (!manifest) return null;

  if (watching) {
    if (state === "idle") return manifest.clips["back/idle"] ? "back/idle" : manifest.states.idle;
    if (["thinking", "working", "listening", "mischief"].includes(state)) {
      return manifest.clips["back/working"] ? "back/working" : manifest.states.working;
    }
    return manifest.clips["back/peek"] ? "back/peek" : manifest.states.peek;
  }

  if (state === "idle" && currentEmotion !== "neutral") {
    return manifest.emotions[currentEmotion] || manifest.states.idle;
  }
  return manifest.states[state] || manifest.states.idle;
}

function renderCurrentState() {
  if (!manifest || activeAction || transient || movementClip || sleeping || draggingPet || wander?.busy) return;
  const clip = clipForState(currentState);
  if (clip && (sprites.clip !== clip || sprites.holding)) sprites.play(clip);
}

function applyState(state) {
  const nextState = effectiveState(state);
  setStateMetadata(nextState);
  renderCurrentState();
}

function setState(state) {
  if (!state) return;
  if (activeAction) {
    pendingStateMessage = { type: "state", state };
    return;
  }
  if (sleeping || transient?.kind === "sleep") {
    startWake(state);
    return;
  }
  applyState(state);
}

function showBubble(text, emotion, speaker = "assistant") {
  if (!text) return;
  currentEmotion = emotion || "neutral";
  body.dataset.emotion = currentEmotion;
  bubble.dataset.speaker = speaker;
  bubbleText.textContent = text;
  bubble.classList.remove("hidden");
  scheduleHitRegions();
  if (bubbleTimer) clearTimeout(bubbleTimer);
  const ms = Math.min(9000, 2500 + text.length * 55);
  bubbleTimer = setTimeout(() => {
    bubble.classList.add("hidden");
    scheduleHitRegions();
    if (speaker === "assistant" && !activeAction && !sleeping) {
      applyState("done");
    }
  }, ms);
}

function reactToWindow(emotion) {
  if (!manifest || activeAction || transient) return;
  const clip = manifest.clips["back/peek"] ? "back/peek" : manifest.states.peek;
  sprites.play(clip);
  sprites.seek(manifest.backReactions[emotion] ?? 0);
}

function resolveActionClip(msg) {
  if (!manifest) return null;
  if (msg.verb === "grab_window") {
    return manifest.windowGestures[msg.gesture || "drag"] || null;
  }
  if (msg.verb === "glitch" && msg.effect) {
    // Her own pranks cast these two with these clips (wander.js), so the command does too.
    const effectClip = { collapse: "action/crack", hijack: "action/glitch" }[msg.effect]
      || `action/${msg.effect}`;
    if (manifest.clips[effectClip]) return effectClip;
  }
  return manifest.actions[msg.verb] || null;
}

function flashAction(msg) {
  if (!manifest) return;
  if (activeAction || draggingPet) {
    pendingAction = msg;
    return;
  }
  if (sleeping || transient?.kind === "sleep") {
    pendingAction = msg;
    startWake("idle");
    return;
  }

  const clip = resolveActionClip(msg);
  if (!clip) return;

  const returnState = currentState === "working" ? "idle" : currentState;
  activeAction = {
    actionId: msg.action_id ?? msg.id ?? null,
    clip,
    returnState,
  };
  transient = null;
  movementClip = null;
  setStateMetadata("working");
  sprites.play(clip, { loop: false });
}

function startSleep() {
  if (!manifest) {
    sleeping = true;
    setStateMetadata("sleep");
    return;
  }
  if (activeAction) {
    pendingStateMessage = { type: "state", state: "idle", corner: true };
    return;
  }
  if (sleeping || transient?.kind === "sleep") return;

  movementClip = null;
  sleeping = false;
  setStateMetadata("sleep");
  transient = { kind: "sleep", clip: "action/sleep", nextState: "idle" };
  sprites.play("action/sleep", { loop: false });
}

function startWake(nextState = "idle", force = false) {
  if (!manifest) {
    sleeping = false;
    applyState(nextState);
    return;
  }
  if (activeAction) {
    pendingStateMessage = { type: "state", state: nextState, wake: true };
    return;
  }
  if (!sleeping && transient?.kind !== "sleep" && !force) {
    applyState(nextState);
    return;
  }

  sleeping = false;
  transient = { kind: "wake", clip: "action/wake", nextState };
  setStateMetadata("wake");
  sprites.play("action/wake", { loop: false });
}

function handleStateMessage(msg) {
  const requested = msg.state || "idle";
  if (activeAction) {
    pendingStateMessage = msg;
    return;
  }
  if (msg.follow === "active-window" && !followActive) setFollow(true);
  if (msg.corner || requested === "sleep" || requested === "sleeping") {
    startSleep();
    return;
  }
  if (msg.wake || requested === "wake" || sleeping || transient?.kind === "sleep") {
    startWake(requested === "wake" ? "idle" : requested, Boolean(msg.wake || requested === "wake"));
    return;
  }
  applyState(requested);
}

function handleConfig(msg) {
  if (typeof msg.mischief === "boolean") {
    mischiefEnabled = msg.mischief;
    mischiefConfigured = true;
    body.dataset.mischief = String(mischiefEnabled);
    if (!activeAction && !sleeping && !transient
        && !mischiefEnabled && currentState === "mischief") {
      applyState("idle");
    }
  }
  if (msg.sleep === true || msg.sleeping === true) startSleep();
  if (msg.wake === true || msg.sleep === false || msg.sleeping === false) startWake("idle", true);
  if (typeof msg.rest_reminder === "boolean") window.HermesRest?.setEnabled(msg.rest_reminder);
}

function handlePoint(msg) {
  if (!manifest) return;
  if (activeAction) {
    pendingStateMessage = { type: "state", state: "peek" };
    return;
  }

  const facing = msg.facing || msg.direction || (watching ? "back" : "front");
  const clip = manifest.points[facing] || manifest.points.back || manifest.points.front;
  if (!clip) return;
  setStateMetadata("peek");
  transient = { kind: "point", clip, nextState: "peek" };
  sprites.play(clip, { loop: false });
}

function clearPendingChat() {
  if (pendingChatTimer) clearTimeout(pendingChatTimer);
  pendingChatTimer = null;
  pendingChat = null;
  form.dataset.sending = "false";
  input.disabled = false;
}

function handleChatResult(msg) {
  if (!pendingChat || (msg.chat_id && msg.chat_id !== pendingChat.id)) return;
  const original = pendingChat.text;
  if (msg.ok === false) {
    input.value = original;
    setStatus(msg.error || "Hermes did not accept the message.", "error",
      typeof msg.copy === "string" ? msg.copy : "");
  } else {
    input.value = "";
    showBubble(original, "neutral", "user");
    setState("listening");
    setStatus("");
  }
  clearPendingChat();
  if (msg.ok === false) input.focus();
  else setChatOpen(false);
  scheduleHitRegions();
}

// The chat box stays hidden until she is ctrl+clicked (or right-clicked, which
// is the same gesture on macOS). Sending a message or Esc closes it again.
function setChatOpen(open) {
  chatOpen = Boolean(open);
  input.classList.toggle("hidden", !chatOpen);
  body.dataset.chat = chatOpen ? "open" : "closed";
  if (chatOpen) {
    input.focus();
    // Ask the plugin what is still missing before typing here can reach your chat.
    sendToPlugin({ type: "chat_check" }).catch(() => {});
  } else {
    input.blur();
  }
  scheduleHitRegions();
}

function handleChatStatus(msg) {
  const steps = Array.isArray(msg.steps)
    ? msg.steps.filter((step) => typeof step === "string" && step.trim())
    : [];
  if (!chatOpen || steps.length === 0 || pendingChat) return;
  const text = steps.length === 1
    ? `To chat with me: ${steps[0]}.`
    : `To chat with me:\n${steps.map((step, i) => `${i + 1}. ${step}`).join("\n")}`;
  setStatus(text, "warning", typeof msg.copy === "string" ? msg.copy : "");
}

function toggleChat() {
  const now = performance.now();
  if (now - chatToggledAt < 400) return; // ctrl+click can fire pointerdown and contextmenu
  chatToggledAt = now;
  setChatOpen(!chatOpen);
}

input.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  e.preventDefault();
  setChatOpen(false);
});

// Ctrl+Shift+click switches her off: the plugin remembers it, her process ends and she
// disappears. /hermeschan on brings her back.
let leaving = false;
function switchOff() {
  if (leaving) return;
  leaving = true;
  invokeNative("deactivate").catch((error) => {
    leaving = false;
    setStatus(`Unable to switch Hermes-chan off: ${errorText(error)}`, "error");
  });
}

chara.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  // On macOS ctrl+click is also a right click, so the same gesture lands here.
  if (e.ctrlKey && e.shiftKey) {
    switchOff();
    return;
  }
  toggleChat();
});

function handle(msg) {
  if (!msg || typeof msg !== "object") return;
  switch (msg.type) {
    case "state":
      handleStateMessage(msg);
      break;
    case "say":
      showBubble(msg.text, msg.emotion, "assistant");
      if (activeAction) {
        pendingStateMessage = { type: "state", state: "speaking" };
      } else if (watching || currentState === "peek" || currentState === "working") {
        setStateMetadata("peek");
        reactToWindow(msg.emotion || "neutral");
      } else {
        setState("speaking");
      }
      break;
    case "emote":
      currentEmotion = msg.emotion || "neutral";
      body.dataset.emotion = currentEmotion;
      if (!activeAction) {
        if (watching || currentState === "peek" || currentState === "working") {
          reactToWindow(currentEmotion);
        } else if (manifest) {
          sprites.play(manifest.emotions[currentEmotion] || manifest.emotions.neutral);
        }
      }
      break;
    case "point":
      handlePoint(msg);
      break;
    case "action":
      flashAction(msg);
      break;
    case "config":
      handleConfig(msg);
      break;
    case "chat_result":
      handleChatResult(msg);
      break;
    case "chat_status":
      handleChatStatus(msg);
      break;
    case "rest_test":
      window.HermesRest?.test(msg.hours);
      break;
    case "action_result":
      if (activeAction && msg.action_id === activeAction.actionId && msg.ok === false) {
        pendingStateMessage = { type: "state", state: "failed" };
        setStatus(msg.message || msg.code || "Desktop action failed.", "error");
      }
      break;
    case "sleep":
      startSleep();
      break;
    case "wake":
      startWake(msg.state || "idle", true);
      break;
    default:
      break;
  }
}

function finishAction(detail) {
  const finished = activeAction;
  if (!finished || detail.clip !== finished.clip) return;
  activeAction = null;

  sendToPlugin({ type: "action_complete", action_id: finished.actionId }).catch((error) => {
    console.warn("Unable to report action completion", error);
  });

  if (pendingAction) {
    const nextAction = pendingAction;
    pendingAction = null;
    flashAction(nextAction);
    return;
  }
  if (pendingStateMessage) {
    const nextState = pendingStateMessage;
    pendingStateMessage = null;
    handleStateMessage(nextState);
    return;
  }
  applyState(finished.returnState || "idle");
}

chara.addEventListener("spriteframe", (e) => {
  const contact = e.detail?.contact;
  if (!contact || !activeAction || e.detail.clip !== activeAction.clip) return;
  sendToPlugin({
    type: "action_event",
    action_id: activeAction.actionId,
    event: contact,
  }).catch((error) => {
    console.warn("Unable to report action frame", error);
  });
});

chara.addEventListener("spritecomplete", (e) => {
  const detail = e.detail || {};
  if (activeAction && detail.clip === activeAction.clip) {
    finishAction(detail);
    return;
  }

  if (transient?.clip === detail.clip) {
    const finished = transient;
    transient = null;
    if (finished.kind === "sleep") {
      sleeping = true;
      setStateMetadata("sleep");
      return;
    }
    if (finished.kind === "wake") {
      sleeping = false;
      if (pendingAction) {
        const nextAction = pendingAction;
        pendingAction = null;
        flashAction(nextAction);
      } else {
        applyState(finished.nextState || "idle");
      }
      return;
    }
    applyState(finished.nextState || currentState);
    return;
  }

  if (movementClip === detail.clip) {
    if (detail.clip === "movement/fall" && manifest?.clips["movement/land"]) {
      movementClip = "movement/land";
      sprites.play("movement/land", { loop: false });
      return;
    }
    movementClip = null;
    if (pendingAction && !activeAction) {
      const nextAction = pendingAction;
      pendingAction = null;
      flashAction(nextAction);
      return;
    }
    renderCurrentState();
    return;
  }

  const stateClip = manifest?.states[currentState];
  if (stateClip === detail.clip && manifest?.clips[stateClip]?.loop === false) {
    const nextState = Object.keys(manifest.states).find((name) => manifest.states[name] === detail.after);
    applyState(nextState || "idle");
  }
});

function setFollowDisplay(active) {
  followActive = Boolean(active);
  if (!followActive) {
    // Unpinned she roams the desktop, so she faces you instead of the active window.
    watching = false;
    body.dataset.watching = "false";
  }
  body.dataset.follow = followActive ? "active" : "manual";
  followToggle.setAttribute("aria-pressed", String(followActive));
  followToggle.textContent = followActive ? "📌" : "○";
  followToggle.title = followActive ? "Following the active window" : "Restore active-window follow";
  scheduleHitRegions();
}

async function setFollow(active) {
  try {
    await invokeNative("set_follow_active", { enabled: active });
    setFollowDisplay(active);
    setStatus("");
    renderCurrentState();
  } catch (error) {
    setStatus(`Unable to change follow mode: ${errorText(error)}`, "error");
  }
}

followToggle.addEventListener("click", () => setFollow(!followActive));

function finishPetDrag() {
  if (!draggingPet) return;
  draggingPet = false;
  chara.classList.remove("dragging");
  const wasDragged = movementClip === "movement/dragged";
  if (wasDragged) movementClip = null;
  // Unpinned, she falls to the screen floor with gravity before landing.
  if (!followActive && wander?.drop(wasDragged)) return;
  if (wasDragged) {
    movementClip = manifest?.clips["movement/land"] ? "movement/land" : null;
    if (movementClip) sprites.play(movementClip, { loop: false });
    else renderCurrentState();
  } else renderCurrentState();
}

window.addEventListener("pointerup", finishPetDrag);
window.addEventListener("mouseup", finishPetDrag);
window.addEventListener("pointermove", (e) => {
  if (draggingPet && e.buttons === 0) finishPetDrag();
});

chara.addEventListener("pointerdown", async (e) => {
  if (e.isPrimary === false || e.button !== 0) return;
  e.preventDefault();
  if (e.ctrlKey && e.shiftKey) {
    switchOff();
    return;
  }
  if (e.ctrlKey) {
    toggleChat();
    return;
  }
  draggingPet = true;
  chara.classList.add("dragging");
  if (!activeAction && !transient && !sleeping && manifest?.clips["movement/dragged"]) {
    movementClip = "movement/dragged";
    sprites.play(movementClip);
  }
  try {
    await invokeNative("drag_overlay");
    await invokeNative("set_follow_active", { enabled: false });
    setFollowDisplay(false);
    setStatus("");
    renderCurrentState();
  } catch (error) {
    setStatus(`Unable to move Hermes-chan: ${errorText(error)}`, "error");
    finishPetDrag();
  }
});

function movementDirection(data) {
  const explicit = String(data.direction || data.move_direction || "").toLowerCase();
  if (explicit === "left" || explicit === "right") return explicit;
  const delta = Number(data.delta_x ?? data.dx);
  if (Number.isFinite(delta) && delta !== 0) return delta < 0 ? "left" : "right";
  return null;
}

function handleActiveWindow(payload) {
  let data;
  try {
    data = parsePayload(payload) || {};
  } catch (_) {
    return;
  }

  const previousWindow = latestActiveWindow;
  latestActiveWindow = data;
  wander?.setActiveWindow(data);
  const reportedWatching = data.watching ?? data.active;
  watching = followActive && (reportedWatching === undefined ? true : Boolean(reportedWatching));
  body.dataset.watching = String(watching);
  if (activeAction || transient || sleeping || draggingPet || wander?.busy || !manifest) return;

  if (!data.direction && !data.move_direction && previousWindow) {
    const previousCenter = Number(previousWindow.x) + Number(previousWindow.w || 0) / 2;
    const currentCenter = Number(data.x) + Number(data.w || 0) / 2;
    if (Number.isFinite(previousCenter) && Number.isFinite(currentCenter)) {
      data = { ...data, delta_x: currentCenter - previousCenter };
    }
  }
  const direction = movementDirection(data);
  const phase = String(data.phase || data.status || "").toLowerCase();
  const moving = data.moving === true || ["moving", "approaching", "before-perch"].includes(phase);
  const deltaY = Number(data.delta_y ?? data.dy);
  if (watching && moving) {
    let clip = null;
    if (Number.isFinite(deltaY) && deltaY > 45 && manifest.clips["movement/fall"]) {
      clip = "movement/fall";
    } else if (Number.isFinite(deltaY) && deltaY < -45) {
      const side = direction || "right";
      clip = manifest.clips[`movement/climb_${side}`] ? `movement/climb_${side}` : null;
    } else if (direction) {
      clip = `movement/walk_${direction}`;
    }
    if (clip && manifest.clips[clip]) {
      if (movementClip === clip && sprites.clip === clip) return;
      movementClip = clip;
      sprites.play(clip, { loop: false });
      return;
    }
  }
  renderCurrentState();
}

function handlePermissionStatus(payload) {
  let data;
  try {
    data = parsePayload(payload);
  } catch (_) {
    return;
  }
  if (typeof data === "boolean") data = { granted: data };
  data = data || {};
  const status = String(data.status || "").toLowerCase();
  if (status === "permission_required") {
    body.dataset.permission = "denied";
    setStatus(data.message || "Accessibility or Screen Recording permission is required.", "warning",
      "", typeof data.settings === "string" ? data.settings : "");
    return;
  }
  if (status === "error") {
    setStatus(data.message || `Window control failed${data.operation ? ` (${data.operation})` : ""}.`, "error");
    return;
  }
  const granted = data.granted ?? data.accessibility ?? data.authorized
    ?? (status === "granted" || status === "ready" ? true : undefined);
  if (granted === undefined) return;
  body.dataset.permission = granted ? "granted" : "denied";
  if (!granted) {
    setStatus(data.message || "Accessibility permission is required for window follow.", "warning");
  } else if (["warning", "error"].includes(statusMessage.dataset.kind)) {
    setStatus("");
  }
}

function interactiveRegions() {
  return [chara, followToggle, bubble, form]
    .filter((element) => element && !element.classList.contains("hidden"))
    .map((element) => element.getBoundingClientRect())
    .filter((rect) => rect.width > 0 && rect.height > 0)
    .map((rect) => ({ x: rect.x, y: rect.y, width: rect.width, height: rect.height }));
}

function scheduleHitRegions() {
  if (hitRegionTimer) cancelAnimationFrame(hitRegionTimer);
  hitRegionTimer = requestAnimationFrame(() => {
    hitRegionTimer = null;
    invokeNative("set_overlay_hit_regions", { regions: interactiveRegions() }).catch(() => {});
  });
}

function listenNative(name, listener) {
  try {
    const registration = event.listen(name, listener);
    if (registration?.catch) {
      registration.catch((error) => console.warn(`Unable to listen for ${name} events`, error));
    }
    return registration;
  } catch (error) {
    console.warn(`Unable to listen for ${name} events`, error);
    return null;
  }
}

if (event?.listen) {
  const fromPlugin = listenNative("from-plugin", (e) => {
    try {
      handle(parsePayload(e.payload));
    } catch (error) {
      console.warn("Ignored malformed plugin message", error);
    }
  });
  // The plugin sends her settings (mischief on/off, asleep…) the moment her body connects,
  // usually before this page listens. Once it does, ask for them again.
  const askForSettings = () => sendToPlugin({ type: "ready" }).catch(() => {});
  Promise.resolve(fromPlugin).then(askForSettings, askForSettings);
  listenNative("active-window", (e) => handleActiveWindow(e.payload));
  // Her rest reminder closed: happy if you clicked it, a little sulky if it timed out.
  listenNative("rest-done", (e) => (e.payload === true
    ? showBubble("Yay ♡ Rest your eyes a little, I'll keep watch!", "love")
    : showBubble("Hmph… at least stretch a little? (｡•́︿•̀｡)", "neutral")));
  listenNative("window-control-status", (e) => handlePermissionStatus(e.payload));
  listenNative("permission-status", (e) => handlePermissionStatus(e.payload));
}

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const original = input.value;
  const text = original.trim();
  if (!text || form.dataset.sending === "true") return;

  const chatId = globalThis.crypto?.randomUUID?.()
    || `chat-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  pendingChat = { id: chatId, text: original };
  form.dataset.sending = "true";
  input.disabled = true;
  setStatus("Sending…");
  try {
    await sendToPlugin({ type: "chat", chat_id: chatId, text });
    pendingChatTimer = setTimeout(() => {
      if (!pendingChat || pendingChat.id !== chatId) return;
      input.value = pendingChat.text;
      clearPendingChat();
      setStatus("Hermes did not acknowledge the message; it was kept here.", "error");
      input.focus();
    }, 3500);
  } catch (error) {
    input.value = original;
    clearPendingChat();
    setStatus(`Message not sent: ${errorText(error)}`, "error");
    input.focus();
  }
});

async function loadOverlayState() {
  try {
    const result = parsePayload(await invokeNative("get_overlay_state")) || {};
    setFollowDisplay(result.follow_active ?? result.followActive ?? false);
    watching = Boolean(result.watching ?? followActive);
    body.dataset.watching = String(watching);
    if (result.permission_status !== undefined) handlePermissionStatus(result.permission_status);
    if (result.active_window !== undefined) handleActiveWindow(result.active_window);
    renderCurrentState();
  } catch (error) {
    setFollowDisplay(false);
    console.warn("Unable to read overlay state", error);
  }
}

if (globalThis.ResizeObserver) {
  const observer = new ResizeObserver(scheduleHitRegions);
  observer.observe(document.body);
  observer.observe(chara);
  observer.observe(form);
}
window.addEventListener("resize", scheduleHitRegions);

function canFall() {
  return Boolean(manifest) && !followActive && !draggingPet && !activeAction;
}

function canRoam() {
  return canFall() && !activeAction && !transient && !movementClip && !sleeping
    && currentState === "idle" && !chatOpen && !pendingChat
    && (bubble.classList.contains("hidden") || bubble.dataset.speaker === "mumble");
}

// Idle mumbles and video reactions from lines.js: local, no model call. Each list is
// shuffled and used up before any line repeats.
const mumbleBags = {};

function nextLine(kind) {
  const lines = window.HermesLines?.[kind];
  if (!lines?.length) return null;
  if (!mumbleBags[kind]?.length) {
    const bag = [...lines];
    for (let i = bag.length - 1; i > 0; i -= 1) {
      const j = Math.floor(Math.random() * (i + 1));
      [bag[i], bag[j]] = [bag[j], bag[i]];
    }
    mumbleBags[kind] = bag;
  }
  return mumbleBags[kind].pop();
}

function canMumble() {
  return Boolean(manifest) && currentState === "idle" && !chatOpen && !pendingChat && !sleeping
    && !draggingPet && !activeAction && bubble.classList.contains("hidden")
    && !["climb", "cling", "jump", "fall", "land", "drop"].includes(wander?.phase);
}

// One line every 40-120 s, or a reaction every 20-60 s while she watches a video with you.
function scheduleMumble() {
  const delay = wander?.watchingVideo ? 20000 + Math.random() * 40000 : 40000 + Math.random() * 80000;
  setTimeout(() => {
    if (canMumble()) {
      const line = nextLine(wander?.watchingVideo ? "video" : "idle");
      if (line) showBubble(line, currentEmotion, "mumble");
    }
    scheduleMumble();
  }, delay);
}

if (window.HermesWander) {
  wander = new window.HermesWander({
    sprites,
    chara,
    canRoam,
    canFall,
    canMischief: () => mischiefEnabled || !mischiefConfigured,
    invoke: invokeNative,
    mood: () => currentEmotion,
    onRelease: () => renderCurrentState(),
  });
}

setStateMetadata("idle");
setFollowDisplay(false);
scheduleHitRegions();
loadOverlayState();
scheduleMumble();
sprites.ready.then(() => {
  manifest = sprites.manifest;
  if (sleeping) {
    sleeping = false;
    startSleep();
  } else {
    applyState(currentState);
  }
}).catch((error) => {
  console.error("Unable to load Hermes-chan sprites", error);
  stateTag.textContent = "sprites unavailable";
});
