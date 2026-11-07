// Her rest reminder takes over the whole screen: the screen glitches and cracks as if she
// hijacked it, then bends into a cute card asking you to rest. One click closes it (and
// she cheers); after 25 seconds it closes by itself. Purely visual, like every effect here:
// nothing underneath is read or touched, and the window never takes keyboard focus.
(function () {
  "use strict";

  const tauri = window.__TAURI__ || {};
  const root = document.getElementById("rest-root");
  const fx = document.getElementById("fx-root");
  if (!root) return;

  const INTRO_MS = 850;
  const AUTO_CLOSE_MS = 25000;
  const BYE_MS = 1500;
  const FLOATERS = ["^_^", "(◕‿◕✿)", "(｡♥‿♥｡)", "♡", "✿", "☆", "(≧◡≦)", "(つ≧▽≦)つ",
    "ʕ•ᴥ•ʔ", "(=^･ω･^=)", "(｡•ᴗ•｡)", "✧", "(っ˘ω˘ς)", "♪", "☕", "(˶ᵔ ᵕ ᵔ˶)", "♡", "✿"];

  let active = false;
  let closing = false;
  let timers = [];
  let player = null;
  // One canvas and one sprite player for every reminder, so none leaves an animation loop behind.
  let chanCanvas = null;

  function words(hours) {
    if (hours >= 9) {
      return { title: `${hours} HOURS!!!`, face: "(´；ω；｀)",
        sub: "U BEEN WORKING FOR SO LONG... I'm worried! Stand up, drink water, look outside ♡",
        clip: "emotion/panic" };
    }
    if (hours >= 6) {
      return { title: `${hours} HOURS?! PLS REST`, face: "(；´Д｀)",
        sub: "U BEEN WORKING FOR SO LONG. Your eyes and your back need a little break ✿",
        clip: "emotion/panic" };
    }
    return { title: "HEY!! TAKE A REST", face: "^_^",
      sub: `U BEEN WORKING FOR SO LONG (${hours} hours straight!) ♡`, clip: "emotion/sleepy" };
  }

  function later(ms, fn) {
    timers.push(setTimeout(fn, ms));
  }

  function clearTimers() {
    timers.forEach(clearTimeout);
    timers = [];
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text != null) node.textContent = text;
    return node;
  }

  // One span per letter so each can bounce on its own beat.
  function bouncy(node, text) {
    node.textContent = "";
    Array.from(text).forEach((char, index) => {
      const span = el("span", char === " " ? "rest-space" : "", char === " " ? "\u00a0" : char);
      span.style.setProperty("--i", String(index));
      node.appendChild(span);
    });
  }

  function floaters() {
    const layer = el("div", "rest-floaters");
    for (let i = 0; i < 26; i += 1) {
      const item = el("span", "rest-floater", FLOATERS[i % FLOATERS.length]);
      item.style.left = `${Math.round(Math.random() * 96)}%`;
      item.style.setProperty("--size", `${Math.round(18 + Math.random() * 34)}px`);
      item.style.setProperty("--delay", `${(Math.random() * 6).toFixed(2)}s`);
      item.style.setProperty("--dur", `${(6 + Math.random() * 6).toFixed(2)}s`);
      item.style.setProperty("--sway", `${Math.round(20 + Math.random() * 60)}px`);
      item.style.setProperty("--hue", String(i % 5));
      layer.appendChild(item);
    }
    const sparkles = el("div", "rest-sparkles");
    for (let i = 0; i < 30; i += 1) {
      const star = el("span", "rest-sparkle", "✦");
      star.style.left = `${Math.round(Math.random() * 100)}%`;
      star.style.top = `${Math.round(Math.random() * 100)}%`;
      star.style.setProperty("--delay", `${(Math.random() * 3).toFixed(2)}s`);
      sparkles.appendChild(star);
    }
    return [layer, sparkles];
  }

  function build(hours) {
    const text = words(hours);
    root.textContent = "";
    root.appendChild(el("div", "rest-tint"));
    floaters().forEach((layer) => root.appendChild(layer));

    const card = el("div", "rest-card");
    if (!chanCanvas) {
      chanCanvas = el("canvas", "rest-chan");
      chanCanvas.width = 192;
      chanCanvas.height = 208;
    }
    const chan = chanCanvas;
    const title = el("h1", "rest-title");
    bouncy(title, text.title);
    const face = el("div", "rest-face", text.face);
    const sub = el("p", "rest-sub", text.sub);
    const hint = el("p", "rest-hint", "click anywhere to take a break ✿");
    card.append(chan, title, face, sub, hint);
    root.appendChild(card);
    return { chan, title, face, sub, hint, clip: text.clip };
  }

  function animateHer(canvas, clip) {
    if (typeof window.HermesSpritePlayer !== "function") return;
    if (!player) player = new window.HermesSpritePlayer(canvas, "hermeschan/");
    player.ready
      .then(() => {
        // The player sizes her to the small on-screen body; here she is big.
        canvas.style.width = "";
        canvas.style.height = "";
        player.play("emotion/surprised", { loop: false });
        later(900, () => player.play(clip, { loop: true }));
      })
      .catch(() => {
        canvas.hidden = true;
      });
  }

  // The "hijack": glitch the whole screen and crack it from the middle.
  function hijack() {
    if (!fx) return;
    fx.setAttribute("data-scanlines", "1");
    fx.setAttribute("data-rgb", "1");
    fx.setAttribute("data-block", "1");
    fx.setAttribute("data-canvas", "crack");
    fx.style.setProperty("--fx-intensity", "0.95");
    window.HermesEffectCanvas?.start("crack", { origin_x: 0.5, origin_y: 0.46, duration: 1500, title: "" });
    later(INTRO_MS + 250, () => {
      fx.removeAttribute("data-scanlines");
      fx.removeAttribute("data-rgb");
      fx.removeAttribute("data-block");
    });
    later(1600, () => {
      fx.removeAttribute("data-canvas");
      fx.style.setProperty("--fx-intensity", "0");
      window.HermesEffectCanvas?.stop();
    });
  }

  function start(payload) {
    let data = payload;
    if (typeof data === "string") {
      try {
        data = JSON.parse(data);
      } catch (_) {
        data = {};
      }
    }
    const hours = Math.max(1, Math.min(99, Math.round(Number(data?.hours)) || 3));
    // A prank in flight must not hide this window when its own timer ends.
    window.HermesEffects?.cancel?.();
    clearTimers();
    active = true;
    closing = false;
    const parts = build(hours);
    root.hidden = false;
    root.dataset.phase = "hijack";
    hijack();
    later(INTRO_MS, () => {
      root.dataset.phase = "card";
      animateHer(parts.chan, parts.clip);
    });
    later(AUTO_CLOSE_MS, () => close(parts, false));
    root.onpointerdown = (event) => {
      event.preventDefault();
      if (root.dataset.phase === "card") close(parts, true);
    };
  }

  function close(parts, promised) {
    if (!active || closing) return;
    closing = true;
    clearTimers();
    root.dataset.phase = "bye";
    bouncy(parts.title, promised ? "YAY! THANK U" : "OK... BUT PLS REST");
    parts.face.textContent = promised ? "(ﾉ◕ヮ◕)ﾉ*:･ﾟ✧" : "(｡•́︿•̀｡)";
    parts.sub.textContent = promised
      ? "Enjoy your break! I'll keep watch ♡"
      : "I'll ask again in a few hours ✿";
    parts.hint.textContent = "";
    player?.play(promised ? "core/celebrate" : "emotion/sulk", { loop: !promised });
    later(BYE_MS - 380, () => root.classList.add("rest-out"));
    later(BYE_MS, () => finish(promised));
  }

  // `promised`: you clicked (true) or it timed out (false); unset when the card failed.
  function finish(promised) {
    clearTimers();
    active = false;
    closing = false;
    root.hidden = true;
    root.classList.remove("rest-out");
    root.onpointerdown = null;
    delete root.dataset.phase;
    root.textContent = "";
    try {
      const pending = tauri.core?.invoke?.("hide_effect",
        typeof promised === "boolean" ? { promised } : {});
      if (pending && typeof pending.catch === "function") pending.catch(() => {});
    } catch (_) {
      // Nothing left to clean up if the bridge is gone.
    }
  }

  tauri.event?.listen?.("rest", (event) => {
    try {
      start(event && "payload" in event ? event.payload : event);
    } catch (_) {
      finish();
    }
  });

  window.HermesRestTakeover = { isActive: () => active, play: start };
})();
