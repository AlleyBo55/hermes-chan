// Rest reminder. After 3 hours at the computer without a real break she takes over the
// screen to ask you to rest, and again at 6, 9, ... hours. Ten minutes away from the
// keyboard and mouse (or the computer sleeping) is a break and starts the count over.
// She only ever reads how long ago your last input was; this window's local storage keeps
// the start and last-active time of the current stretch, so a restart doesn't reset it.
(function () {
  "use strict";

  const core = window.__TAURI__?.core;
  const KEY = "hermeschan.rest.v1";
  const POLL_MS = 30 * 1000;
  const EVERY_HOURS = 3;
  const BREAK_SECONDS = 10 * 60; // away this long: a real break
  const ACTIVE_SECONDS = 5 * 60; // shorter pauses (reading, thinking) still count as work
  const HOUR_MS = 3600 * 1000;

  function freshState() {
    return { start: 0, last: 0, reminded: 0 };
  }

  // Pure: fold one idle sample into the stretch. Returns the hours to remind about, or 0.
  function step(state, idleSeconds, now) {
    const gapMs = state.last > 0 ? now - state.last : 0;
    if (idleSeconds >= BREAK_SECONDS || gapMs >= BREAK_SECONDS * 1000) {
      Object.assign(state, freshState());
    }
    if (idleSeconds < ACTIVE_SECONDS) {
      if (!state.start) state.start = now - idleSeconds * 1000;
      state.last = now;
    }
    if (!state.start) return 0;
    const due = Math.floor((state.last - state.start) / HOUR_MS / EVERY_HOURS) * EVERY_HOURS;
    if (due >= EVERY_HOURS && due > state.reminded) {
      state.reminded = due;
      return due;
    }
    return 0;
  }

  function load() {
    try {
      const raw = JSON.parse(localStorage.getItem(KEY) || "null");
      if (raw && Number.isFinite(raw.start) && Number.isFinite(raw.last)) {
        return { start: raw.start, last: raw.last, reminded: Number(raw.reminded) || 0 };
      }
    } catch (_) {
      // Unreadable storage: start a fresh stretch.
    }
    return freshState();
  }

  function save(state) {
    try {
      localStorage.setItem(KEY, JSON.stringify(state));
    } catch (_) {
      // Storage full or disabled: the stretch just won't survive a restart.
    }
  }

  let enabled = true;
  const state = load();

  function show(hours, test) {
    if (!core?.invoke) return;
    window.dispatchEvent(new CustomEvent("hermeschan-rest", { detail: { hours } }));
    core.invoke("show_rest_reminder", { hours, test: Boolean(test) })
      .catch((error) => console.warn("Hermes-chan could not show her rest reminder", error));
  }

  async function tick() {
    if (!core?.invoke) return;
    let idle;
    try {
      idle = await core.invoke("user_idle_seconds");
    } catch (_) {
      return;
    }
    // null: this desktop can't tell how long you've been away, so she can't see your
    // breaks either. Better quiet than nagging you while you rest.
    if (typeof idle !== "number" || !Number.isFinite(idle)) return;
    const due = step(state, idle, Date.now());
    save(state);
    if (due && enabled) show(due, false);
  }

  if (typeof module !== "undefined") {
    // Node, for the simulation in development: expose the pure core only.
    module.exports = { step, freshState, EVERY_HOURS, BREAK_SECONDS, ACTIVE_SECONDS };
    return;
  }

  window.HermesRest = {
    setEnabled(value) {
      enabled = Boolean(value);
    },
    test(hours) {
      const value = Math.round(Number(hours));
      show(Number.isFinite(value) ? Math.max(1, Math.min(99, value)) : EVERY_HOURS, true);
    },
  };
  setTimeout(tick, 5000);
  setInterval(tick, POLL_MS);
})();
