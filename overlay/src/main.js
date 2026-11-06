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
let followActive = false;
let watching = false;
let latestActiveWindow = null;
let pendingChat = null;
let pendingChatTimer = null;
let hitRegionTimer = null;

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

function setStatus(text, kind = "") {
  statusMessage.textContent = text || "";
  statusMessage.dataset.kind = kind;
  statusMessage.classList.toggle("hidden", !text);
  scheduleHitRegions();
}

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
  if (!manifest || activeAction || transient || movementClip || sleeping) return;
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
  return manifest.actions[msg.verb] || null;
}

function flashAction(msg) {
  if (!manifest) return;
  if (activeAction) {
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
    body.dataset.mischief = String(mischiefEnabled);
    if (!activeAction && !sleeping && !transient
        && !mischiefEnabled && currentState === "mischief") {
      applyState("idle");
    }
  }
  if (msg.sleep === true || msg.sleeping === true) startSleep();
  if (msg.wake === true || msg.sleep === false || msg.sleeping === false) startWake("idle", true);
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
    setStatus(msg.error || "Hermes did not accept the message.", "error");
  } else {
    input.value = "";
    showBubble(original, "neutral", "user");
    setState("listening");
    setStatus("");
  }
  clearPendingChat();
  input.focus();
  scheduleHitRegions();
}

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

chara.addEventListener("pointerdown", async (e) => {
  if (e.isPrimary === false || e.button !== 0) return;
  e.preventDefault();
  chara.classList.add("dragging");
  try {
    await invokeNative("drag_overlay");
    await invokeNative("set_follow_active", { enabled: false });
    setFollowDisplay(false);
    setStatus("");
    renderCurrentState();
  } catch (error) {
    setStatus(`Unable to move Hermes-chan: ${errorText(error)}`, "error");
  } finally {
    chara.classList.remove("dragging");
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
  const reportedWatching = data.watching ?? data.active;
  watching = reportedWatching === undefined ? true : Boolean(reportedWatching);
  body.dataset.watching = String(watching);
  if (activeAction || transient || sleeping || !manifest) return;

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
    setStatus(data.message || "Accessibility or Screen Recording permission is required.", "warning");
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
  } catch (error) {
    console.warn(`Unable to listen for ${name} events`, error);
  }
}

if (event?.listen) {
  listenNative("from-plugin", (e) => {
    try {
      handle(parsePayload(e.payload));
    } catch (error) {
      console.warn("Ignored malformed plugin message", error);
    }
  });
  listenNative("active-window", (e) => handleActiveWindow(e.payload));
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

setStateMetadata("idle");
setFollowDisplay(false);
scheduleHitRegions();
loadOverlayState();
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
