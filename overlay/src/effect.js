// Hermeschan effect overlay front-end. No bundler, no npm: Tauri exposes
// globals via `withGlobalTauri: true`. This window is a transparent, purely
// decorative surface. It listens for the Tauri event "effect", renders a
// bounded, harmless distortion (scanlines / RGB split / block glitch / bend
// border), and calls the Rust command `hide_effect` (no args) when the
// animation ends. It never reads, writes, or injects input into any target
// app — every layer is CSS-only visual noise over a transparent page.
//
// Robust by design: if the Tauri globals are missing (e.g. opened in a plain
// browser for preview) nothing throws — listeners and the invoke simply no-op.

(function () {
  "use strict";

  var tauri = (typeof window !== "undefined" && window.__TAURI__) || {};
  var event = tauri.event || null;
  var core = tauri.core || null;

  var root = document.getElementById("fx-root");

  // Known effect keys -> the data-attribute the CSS keys off.
  var EFFECT_MAP = {
    scanline: "scanlines",
    scanlines: "scanlines",
    scan: "scanlines",
    rgb: "rgb",
    "rgb-split": "rgb",
    rgbsplit: "rgb",
    chromatic: "rgb",
    block: "block",
    blocks: "block",
    "block-glitch": "block",
    bend: "bend",
    "bend-border": "bend",
    border: "bend",
    distort: "bend",
    crack: "crack",
    "screen-crack": "crack",
    shatter: "crack",
    portal: "portal",
    rift: "portal",
    text_shatter: "text_shatter",
    "font-break": "text_shatter",
    glyphs: "text_shatter",
    ripple: "ripple",
    wave: "ripple",
    pixel_dissolve: "pixel_dissolve",
    pixels: "pixel_dissolve",
    lightning: "lightning",
    electric: "lightning",
    tear: "tear",
    rip: "tear",
    collapse: "collapse",
    "ui-break": "collapse"
  };

  var DEFAULT_DURATION_MS = 1200;
  var MIN_DURATION_MS = 100;
  var MAX_DURATION_MS = 15000; // hard cap: the animation is always bounded.

  var activeTimer = null;
  var running = false;

  function clamp(n, lo, hi) {
    if (typeof n !== "number" || isNaN(n)) return null;
    return Math.max(lo, Math.min(hi, n));
  }

  // Normalize an arbitrary payload (string or object) into a plain config.
  function parsePayload(payload) {
    var data = payload;
    if (typeof payload === "string") {
      var trimmed = payload.trim();
      if (trimmed.charAt(0) === "{" || trimmed.charAt(0) === "[") {
        try {
          data = JSON.parse(trimmed);
        } catch (_) {
          data = trimmed; // not JSON: treat the string itself as an effect name
        }
      } else {
        data = trimmed;
      }
    }

    var names = [];
    if (typeof data === "string") {
      names = [data];
    } else if (Array.isArray(data)) {
      names = data;
    } else if (data && typeof data === "object") {
      if (Array.isArray(data.effects)) {
        names = data.effects;
      } else if (data.effect != null) {
        names = [data.effect];
      } else if (data.name != null) {
        names = [data.name];
      } else if (data.type != null && data.type !== "effect") {
        names = [data.type];
      }
    }

    // Resolve names (also splitting comma/space separated strings) to CSS keys.
    var keys = {};
    names.forEach(function (raw) {
      if (raw == null) return;
      String(raw)
        .split(/[\s,]+/)
        .forEach(function (token) {
          token = token.toLowerCase().trim();
          if (token === "glitch") {
            keys.scanlines = true;
            keys.rgb = true;
            keys.block = true;
            return;
          }
          var key = EFFECT_MAP[token];
          if (key) keys[key] = true;
        });
    });

    var intensity = 0.85;
    var duration = DEFAULT_DURATION_MS;
    var origin_x = 0.5;
    var origin_y = 0.35;
    var title = "ACTIVE APP";
    if (data && typeof data === "object" && !Array.isArray(data)) {
      var ci = clamp(Number(data.intensity), 0, 1);
      if (ci != null) intensity = ci;
      var cd = clamp(Number(data.duration_ms), MIN_DURATION_MS, MAX_DURATION_MS);
      if (cd != null) duration = cd;
      var cx = clamp(Number(data.origin_x), 0, 1);
      var cy = clamp(Number(data.origin_y), 0, 1);
      if (cx != null) origin_x = cx;
      if (cy != null) origin_y = cy;
      title = String(data.title || data.app || title).slice(0, 28);
    }

    return { keys: keys, intensity: intensity, duration: duration,
             origin_x: origin_x, origin_y: origin_y, title: title };
  }

  function clearLayers() {
    if (!root) return;
    root.removeAttribute("data-scanlines");
    root.removeAttribute("data-rgb");
    root.removeAttribute("data-block");
    root.removeAttribute("data-bend");
    root.removeAttribute("data-canvas");
    root.style.setProperty("--fx-intensity", "0");
    window.HermesEffectCanvas?.stop();
  }

  function endEffect() {
    if (activeTimer) {
      clearTimeout(activeTimer);
      activeTimer = null;
    }
    clearLayers();
    running = false;
    // Tell the Rust side to hide/close this window. No arguments.
    if (core && typeof core.invoke === "function") {
      try {
        var p = core.invoke("hide_effect");
        if (p && typeof p.catch === "function") p.catch(function () {});
      } catch (_) {
        /* ignore: absent/failed command must not throw */
      }
    }
  }

  // Stop a running prank without hiding the window (her rest reminder is taking it over).
  function cancelEffect() {
    if (activeTimer) {
      clearTimeout(activeTimer);
      activeTimer = null;
    }
    clearLayers();
    running = false;
  }

  function runEffect(payload) {
    if (!root) return;
    if (window.HermesRestTakeover && window.HermesRestTakeover.isActive()) return;
    var cfg = parsePayload(payload);

    // Nothing recognized -> still honor the contract: show nothing briefly and
    // hand control back so the window gets hidden rather than lingering.
    var hasAny = Object.keys(cfg.keys).length > 0;

    // Cancel any in-flight run and start fresh.
    if (activeTimer) {
      clearTimeout(activeTimer);
      activeTimer = null;
    }
    clearLayers();

    if (cfg.keys.scanlines) root.setAttribute("data-scanlines", "1");
    if (cfg.keys.rgb) root.setAttribute("data-rgb", "1");
    if (cfg.keys.block) root.setAttribute("data-block", "1");
    if (cfg.keys.bend) root.setAttribute("data-bend", "1");
    var canvasEffect = ["portal", "crack", "text_shatter", "ripple",
                        "pixel_dissolve", "lightning", "tear", "collapse"]
      .find(function (name) { return cfg.keys[name]; });
    if (canvasEffect) root.setAttribute("data-canvas", canvasEffect);
    root.style.setProperty("--fx-intensity", String(hasAny ? cfg.intensity : 0));
    if (canvasEffect) {
      window.HermesEffectCanvas?.start(canvasEffect, cfg);
    }

    running = true;
    var ms = hasAny ? cfg.duration : MIN_DURATION_MS;
    activeTimer = setTimeout(endEffect, ms);
  }

  if (event && typeof event.listen === "function") {
    // The snapshot for the "collapse" prank arrives just before its effect.
    event.listen("effect-image", function (e) {
      if (typeof e.payload === "string" && window.HermesEffectCanvas?.setSnapshot) {
        window.HermesEffectCanvas.setSnapshot("data:image/png;base64," + e.payload);
      }
    });
    event.listen("effect", function (e) {
      try {
        runEffect(e && "payload" in e ? e.payload : e);
      } catch (_) {
        // Any unexpected payload must not wedge the window open.
        endEffect();
      }
    });
  }

  // Used by the local visual preview; production still receives Tauri events.
  window.HermesEffects = { play: runEffect, stop: endEffect, cancel: cancelEffect };
})();
