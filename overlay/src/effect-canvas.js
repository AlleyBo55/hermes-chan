// Animated, click-through crack and portal art for the active-window effect.
// It draws only into this transparent overlay; the app underneath is untouched.
(function () {
  "use strict";

  const canvas = document.getElementById("fx-canvas");
  const ctx = canvas?.getContext("2d");
  if (!ctx) return;

  const TAU = Math.PI * 2;
  let active = null;
  let animationFrame = 0;
  let width = 1;
  let height = 1;

  const clamp = (value, low, high) => Math.max(low, Math.min(high, value));
  const easeOut = (value) => 1 - (1 - value) ** 3;

  function resize() {
    width = Math.max(1, window.innerWidth);
    height = Math.max(1, window.innerHeight);
    const ratio = Math.min(window.devicePixelRatio || 1, 2, 4096 / width, 4096 / height);
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  }

  function random(seed) {
    let state = seed >>> 0;
    return () => {
      state = (1664525 * state + 1013904223) >>> 0;
      return state / 4294967296;
    };
  }

  function makeCracks(cx, cy) {
    const rand = random(Math.round(cx * 31 + cy * 73 + width * 7 + height * 11));
    const radius = clamp(Math.min(width, height) * 0.62, 140, 560);
    const rays = [];
    const branches = [];
    for (let ray = 0; ray < 15; ray += 1) {
      const angle = (ray / 15) * TAU + (rand() - 0.5) * 0.2;
      const length = radius * (0.68 + rand() * 0.45);
      const points = [{ x: cx, y: cy }];
      for (let step = 1; step <= 5; step += 1) {
        const distance = (step / 5) * length;
        const bend = (rand() - 0.5) * 0.24;
        points.push({
          x: cx + Math.cos(angle + bend) * distance,
          y: cy + Math.sin(angle + bend) * distance,
        });
      }
      rays.push(points);
      const fork = points[2 + (ray % 2)];
      const branchAngle = angle + (ray % 2 ? 0.8 : -0.8);
      branches.push([
        fork,
        { x: fork.x + Math.cos(branchAngle) * length * 0.18,
          y: fork.y + Math.sin(branchAngle) * length * 0.18 },
        { x: fork.x + Math.cos(branchAngle + 0.13) * length * 0.36,
          y: fork.y + Math.sin(branchAngle + 0.13) * length * 0.36 },
      ]);
    }
    return { rays, branches, radius };
  }

  function partialPath(points, progress) {
    const step = clamp(progress, 0, 1) * (points.length - 1);
    const whole = Math.floor(step);
    ctx.beginPath();
    ctx.moveTo(points[0].x, points[0].y);
    for (let index = 1; index <= whole; index += 1) {
      ctx.lineTo(points[index].x, points[index].y);
    }
    if (whole < points.length - 1) {
      const fraction = step - whole;
      const from = points[whole];
      const to = points[whole + 1];
      ctx.lineTo(from.x + (to.x - from.x) * fraction,
                 from.y + (to.y - from.y) * fraction);
    }
  }

  function drawCrack(fx, progress) {
    const reveal = easeOut(clamp(progress / 0.34, 0, 1));
    const fade = clamp((1 - progress) / 0.22, 0, 1);
    ctx.save();
    ctx.globalAlpha = fade;
    const flash = clamp(1 - progress / 0.28, 0, 1);
    if (flash > 0) {
      const glow = ctx.createRadialGradient(fx.cx, fx.cy, 0, fx.cx, fx.cy, fx.paths.radius * 0.44);
      glow.addColorStop(0, `rgba(238,252,255,${0.5 * flash})`);
      glow.addColorStop(0.35, `rgba(100,210,255,${0.22 * flash})`);
      glow.addColorStop(1, "rgba(100,210,255,0)");
      ctx.fillStyle = glow;
      ctx.fillRect(0, 0, width, height);
    }
    for (const [paths, delay, lineWidth] of [
      [fx.paths.rays, 0, 2.7],
      [fx.paths.branches, 0.12, 1.5],
    ]) {
      const amount = clamp((reveal - delay) / (1 - delay), 0, 1);
      for (const points of paths) {
        partialPath(points, amount);
        ctx.lineJoin = "round";
        ctx.strokeStyle = "rgba(6,12,30,0.92)";
        ctx.lineWidth = lineWidth + 1;
        ctx.stroke();
        ctx.save();
        ctx.translate(-1.2, -1.2);
        partialPath(points, amount);
        ctx.strokeStyle = "rgba(220,248,255,0.95)";
        ctx.lineWidth = lineWidth;
        ctx.shadowColor = "#83e6ff";
        ctx.shadowBlur = 7;
        ctx.stroke();
        ctx.restore();
      }
    }
    if (progress < 0.38) {
      ctx.beginPath();
      ctx.arc(fx.cx, fx.cy, fx.paths.radius * 0.32 * easeOut(progress / 0.38), 0, TAU);
      ctx.strokeStyle = `rgba(170,235,255,${0.7 * (1 - progress / 0.38)})`;
      ctx.lineWidth = 3;
      ctx.stroke();
    }
    ctx.restore();
  }

  function drawPortal(fx, progress, now) {
    const opening = easeOut(clamp(progress / 0.25, 0, 1));
    const closing = easeOut(clamp((1 - progress) / 0.19, 0, 1));
    const open = Math.min(opening, closing);
    if (open <= 0) return;
    const radius = fx.radius * open;
    const spin = (now - fx.started) / 900;
    ctx.save();
    ctx.translate(fx.cx, fx.cy);
    ctx.scale(1, 0.78);

    const halo = ctx.createRadialGradient(0, 0, radius * 0.62, 0, 0, radius * 1.38);
    halo.addColorStop(0, "rgba(19,7,47,0.88)");
    halo.addColorStop(0.65, "rgba(54,36,119,0.8)");
    halo.addColorStop(0.82, "rgba(27,184,255,0.43)");
    halo.addColorStop(1, "rgba(27,184,255,0)");
    ctx.fillStyle = halo;
    ctx.beginPath();
    ctx.arc(0, 0, radius * 1.38, 0, TAU);
    ctx.fill();

    const voidFill = ctx.createRadialGradient(-radius * 0.15, -radius * 0.15, 0, 0, 0, radius);
    voidFill.addColorStop(0, "rgba(3,5,24,0.98)");
    voidFill.addColorStop(0.62, "rgba(20,9,57,0.96)");
    voidFill.addColorStop(1, "rgba(58,36,130,0.94)");
    ctx.fillStyle = voidFill;
    ctx.beginPath();
    ctx.arc(0, 0, radius * 0.92, 0, TAU);
    ctx.fill();

    for (let spiral = 0; spiral < 9; spiral += 1) {
      ctx.beginPath();
      for (let point = 0; point <= 24; point += 1) {
        const fraction = point / 24;
        const angle = spiral * TAU / 9 + spin * 1.6 + fraction * 2.5;
        const distance = radius * (0.12 + fraction * 0.72);
        const x = Math.cos(angle) * distance;
        const y = Math.sin(angle) * distance;
        if (point === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.strokeStyle = spiral % 2 ? "rgba(93,81,230,0.42)" : "rgba(75,213,255,0.38)";
      ctx.lineWidth = 2.4;
      ctx.stroke();
    }

    ctx.lineCap = "round";
    for (let ring = 0; ring < 4; ring += 1) {
      const start = spin * (ring % 2 ? -1 : 1) + ring * 1.3;
      ctx.beginPath();
      ctx.arc(0, 0, radius * (0.96 + ring * 0.045), start, start + 1.8 + ring * 0.17);
      ctx.strokeStyle = ["#9df3ff", "#6acbff", "#a187ff", "#e5c1ff"][ring];
      ctx.lineWidth = Math.max(2, radius * (0.045 - ring * 0.006));
      ctx.shadowColor = ring % 2 ? "#a076ff" : "#5ce5ff";
      ctx.shadowBlur = radius * 0.17;
      ctx.stroke();
    }
    ctx.shadowBlur = 0;
    for (let spark = 0; spark < 18; spark += 1) {
      const angle = spark * TAU / 18 + spin * (spark % 2 ? 0.5 : -0.4);
      const distance = radius * (1.06 + 0.07 * Math.sin(spin * 3 + spark));
      ctx.beginPath();
      ctx.arc(Math.cos(angle) * distance, Math.sin(angle) * distance,
              Math.max(1.3, radius * 0.012), 0, TAU);
      ctx.fillStyle = spark % 3 ? "rgba(153,231,255,0.9)" : "rgba(221,172,255,0.9)";
      ctx.fill();
    }
    ctx.restore();
  }

  function makeGlyphs(cx, cy, title) {
    const letters = Array.from(String(title || "ACTIVE APP").slice(0, 28));
    const fontSize = clamp(width / Math.max(letters.length * 0.65, 12), 18, 32);
    ctx.font = `800 ${fontSize}px system-ui, sans-serif`;
    const total = letters.reduce((sum, letter) => sum + ctx.measureText(letter).width, 0);
    const rand = random(letters.length * 97 + Math.round(cx * 11 + cy * 17));
    let x = cx - total / 2;
    return {
      fontSize,
      letters: letters.map((letter) => {
        const letterWidth = ctx.measureText(letter).width;
        const glyph = {
          letter,
          x: x + letterWidth / 2,
          y: cy,
          dx: (rand() - 0.5) * 230,
          dy: -35 - rand() * 155,
          spin: (rand() - 0.5) * 2.8,
        };
        x += letterWidth;
        return glyph;
      }),
    };
  }

  function drawTextShatter(fx, progress) {
    const scatter = easeOut(clamp((progress - 0.14) / 0.62, 0, 1));
    const fade = clamp((1 - progress) / 0.25, 0, 1);
    ctx.save();
    ctx.globalAlpha = fade;
    ctx.font = `800 ${fx.glyphs.fontSize}px system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const [index, glyph] of fx.glyphs.letters.entries()) {
      ctx.save();
      ctx.translate(glyph.x + glyph.dx * scatter,
                    glyph.y + glyph.dy * scatter + 110 * scatter * scatter);
      ctx.rotate(glyph.spin * scatter);
      ctx.lineWidth = 4;
      ctx.strokeStyle = "rgba(8,17,42,0.94)";
      ctx.strokeText(glyph.letter, 0, 0);
      ctx.shadowColor = index % 2 ? "#8a7dff" : "#5ce5ff";
      ctx.shadowBlur = 12;
      ctx.fillStyle = index % 3 ? "#dffaff" : "#a9a0ff";
      ctx.fillText(glyph.letter, 0, 0);
      if (scatter > 0.1 && glyph.letter.trim()) {
        ctx.fillStyle = "rgba(155,231,255,0.82)";
        ctx.fillRect(-2, -fx.glyphs.fontSize * 0.65 - scatter * 12, 3, 5);
      }
      ctx.restore();
    }
    ctx.restore();
  }

  function drawRipple(fx, progress) {
    const radius = Math.min(Math.hypot(width, height) * 0.62, 850);
    ctx.save();
    for (let ring = 0; ring < 5; ring += 1) {
      const phase = progress * 1.36 - ring * 0.16;
      if (phase <= 0 || phase >= 1) continue;
      const reach = radius * easeOut(phase);
      const alpha = (1 - phase) * 0.78;
      ctx.beginPath();
      ctx.arc(fx.cx, fx.cy, reach, 0, TAU);
      ctx.strokeStyle = `rgba(108,222,255,${alpha})`;
      ctx.lineWidth = 2 + (1 - phase) * 7;
      ctx.shadowColor = "#78dfff";
      ctx.shadowBlur = 18;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(fx.cx, fx.cy, reach + 7, Math.PI * 0.15, Math.PI * 1.36);
      ctx.strokeStyle = `rgba(195,167,255,${alpha * 0.54})`;
      ctx.lineWidth = 2;
      ctx.stroke();
    }
    ctx.restore();
  }

  function makePixels(cx, cy) {
    const rand = random(Math.round(cx * 43 + cy * 61));
    const pixels = [];
    for (let row = -9; row <= 9; row += 1) {
      for (let column = -14; column <= 14; column += 1) {
        if (rand() < 0.27) continue;
        const x = cx + column * 30;
        const y = cy + row * 30;
        const distance = Math.hypot(column / 14, row / 9);
        pixels.push({
          x, y, size: 14 + rand() * 18,
          dx: column * (9 + rand() * 12) + (rand() - 0.5) * 70,
          dy: row * (9 + rand() * 12) - rand() * 56,
          delay: distance * 0.18 + rand() * 0.12,
          color: ["#a7ebff", "#6eaeff", "#b395ff", "#122445"][Math.floor(rand() * 4)],
        });
      }
    }
    return pixels;
  }

  function drawPixelDissolve(fx, progress) {
    ctx.save();
    for (const pixel of fx.pixels) {
      const scatter = clamp((progress - pixel.delay) / 0.74, 0, 1);
      const fade = clamp((1 - progress) / 0.3, 0, 1) * (1 - scatter * 0.25);
      ctx.globalAlpha = fade;
      ctx.fillStyle = pixel.color;
      ctx.fillRect(pixel.x + pixel.dx * scatter, pixel.y + pixel.dy * scatter,
                   pixel.size * (1 - scatter * 0.55), pixel.size * (1 - scatter * 0.55));
    }
    ctx.restore();
  }

  function makeLightning(cx, cy) {
    const rand = random(Math.round(cx * 31 + cy * 83 + width * 3));
    const endpoints = [
      [width * 0.05, height * 0.08], [width * 0.95, height * 0.1],
      [width * 0.06, height * 0.92], [width * 0.94, height * 0.9],
      [width * 0.5, height * 0.03], [width * 0.52, height * 0.97],
    ];
    return endpoints.map(([endX, endY]) => {
      const dx = endX - cx;
      const dy = endY - cy;
      const distance = Math.hypot(dx, dy) || 1;
      const points = [{ x: cx, y: cy }];
      for (let step = 1; step < 11; step += 1) {
        const fraction = step / 11;
        const offset = (rand() - 0.5) * Math.min(distance * 0.12, 45);
        points.push({ x: cx + dx * fraction - dy / distance * offset,
                      y: cy + dy * fraction + dx / distance * offset });
      }
      points.push({ x: endX, y: endY });
      return points;
    });
  }

  function drawLightning(fx, progress) {
    const reveal = easeOut(clamp(progress / 0.27, 0, 1));
    const fade = clamp((1 - progress) / 0.31, 0, 1);
    const flicker = 0.75 + 0.25 * Math.sin(progress * 91) ** 2;
    ctx.save();
    ctx.globalAlpha = fade * flicker;
    for (const bolt of fx.bolts) {
      partialPath(bolt, reveal);
      ctx.lineJoin = "round";
      ctx.strokeStyle = "#478aff";
      ctx.lineWidth = 8;
      ctx.shadowColor = "#72d9ff";
      ctx.shadowBlur = 23;
      ctx.stroke();
      partialPath(bolt, reveal);
      ctx.strokeStyle = "#e3faff";
      ctx.lineWidth = 2.4;
      ctx.shadowBlur = 3;
      ctx.stroke();
    }
    ctx.restore();
  }

  function makeTear(cx, cy) {
    const rand = random(Math.round(cx * 19 + cy * 37));
    const span = Math.min(width * 0.88, 850);
    const points = [];
    for (let index = 0; index <= 22; index += 1) {
      points.push({
        x: clamp(cx - span / 2 + span * index / 22, 0, width),
        y: cy + (rand() - 0.5) * 20,
      });
    }
    return points;
  }

  function drawTear(fx, progress) {
    const opening = easeOut(clamp(progress / 0.31, 0, 1));
    const closing = easeOut(clamp((1 - progress) / 0.22, 0, 1));
    const gap = Math.min(65, height * 0.12) * Math.min(opening, closing);
    if (gap <= 0) return;
    ctx.save();
    ctx.beginPath();
    fx.tear.forEach((point, index) => {
      const y = point.y - gap * Math.sin(Math.PI * index / 22);
      if (index === 0) ctx.moveTo(point.x, y);
      else ctx.lineTo(point.x, y);
    });
    for (let index = fx.tear.length - 1; index >= 0; index -= 1) {
      const point = fx.tear[index];
      ctx.lineTo(point.x, point.y + gap * Math.sin(Math.PI * index / 22));
    }
    ctx.closePath();
    const fill = ctx.createLinearGradient(0, fx.cy - gap, 0, fx.cy + gap);
    fill.addColorStop(0, "rgba(36,20,83,0.96)");
    fill.addColorStop(0.5, "rgba(3,4,18,0.99)");
    fill.addColorStop(1, "rgba(42,17,92,0.96)");
    ctx.fillStyle = fill;
    ctx.fill();
    for (const direction of [-1, 1]) {
      ctx.beginPath();
      fx.tear.forEach((point, index) => {
        const y = point.y + direction * gap * Math.sin(Math.PI * index / 22);
        if (index === 0) ctx.moveTo(point.x, y);
        else ctx.lineTo(point.x, y);
      });
      ctx.strokeStyle = direction < 0 ? "#b6f0ff" : "#b99dff";
      ctx.lineWidth = 3;
      ctx.shadowColor = direction < 0 ? "#69dcff" : "#a271ff";
      ctx.shadowBlur = 13;
      ctx.stroke();
    }
    ctx.restore();
  }

  // "Her prank broke the app": a snapshot of the area cracks, its pieces fall into a dark
  // void, then everything fades and the real (untouched) interface is back. The snapshot
  // arrives from Rust just before the effect starts and is drawn only here.
  let snapshot = null;
  let snapshotAt = -Infinity;

  function setSnapshot(src) {
    // Never draw the previous window's picture while this one decodes.
    snapshot = null;
    const image = new Image();
    image.onload = () => {
      snapshot = image;
      snapshotAt = performance.now();
    };
    image.src = src;
  }

  function makeTiles(seed) {
    const rnd = random(seed);
    const cols = 9;
    const rows = 7;
    const tiles = [];
    for (let r = 0; r < rows; r += 1) {
      for (let c = 0; c < cols; c += 1) {
        tiles.push({ c, r, cols, rows, vx: (rnd() - 0.5) * 220, vy: -rnd() * 160,
          spin: (rnd() - 0.5) * 4, delay: rnd() * 0.25 + (1 - r / rows) * 0.1 });
      }
    }
    return tiles;
  }

  function drawCollapse(fx, progress) {
    const image = snapshot;
    if (!image) return;
    const fade = progress > 0.82 ? 1 - (progress - 0.82) / 0.18 : 1;
    ctx.save();
    ctx.globalAlpha = fade;
    if (progress > 0.12) {
      ctx.fillStyle = "rgba(6, 8, 16, 0.94)";
      ctx.fillRect(0, 0, width, height);
    }
    const tw = width / fx.tiles[0].cols;
    const th = height / fx.tiles[0].rows;
    const iw = image.width / fx.tiles[0].cols;
    const ih = image.height / fx.tiles[0].rows;
    for (const tile of fx.tiles) {
      const fall = clamp((progress - 0.12 - tile.delay) / 0.62, 0, 1) * 1.5; // seconds of fall
      const shake = progress < 0.12 ? Math.sin(progress * 260 + tile.c) * 2 : 0;
      ctx.save();
      ctx.translate(tile.c * tw + tw / 2 + tile.vx * fall + shake, tile.r * th + th / 2 + tile.vy * fall + 900 * fall * fall);
      ctx.rotate(tile.spin * fall);
      ctx.drawImage(image, tile.c * iw, tile.r * ih, iw, ih, -tw / 2, -th / 2, tw + 1, th + 1);
      ctx.restore();
    }
    ctx.restore();
    if (progress < 0.2) drawCrack(fx, clamp(progress / 0.12, 0, 1));
  }

  // "She hijacked your app": from her hand the whole window glitches out (its real pixels
  // slip sideways, split into colour fringes and break into blocks), then switches off like
  // an old screen. Without a fresh snapshot (no Screen Recording) it gets drawn-on
  // corruption instead of the window's pixels.
  const HIJACK_STEP_MS = 70;
  const HIJACK_LABEL = "HIJACKED BY HERMES-CHAN (^_~)";
  const NEON = ["#ff2bd6", "#28f0ff", "#7dff5a", "#ffe14d", "#120018"];
  const scratch = document.createElement("canvas");
  const sctx = scratch.getContext("2d");

  function makeHijack(cx, cy) {
    return { seed: (Math.floor(cx * 13 + cy * 29) + Math.floor(performance.now())) >>> 0 };
  }

  // One band of the snapshot in a single colour channel, for the colour fringes.
  function tint(image, x, y, w, h, color) {
    const sx = image.width / width;
    const sy = image.height / height;
    if (scratch.width < w) scratch.width = Math.ceil(w);
    if (scratch.height < h) scratch.height = Math.ceil(h);
    sctx.globalCompositeOperation = "copy";
    sctx.drawImage(image, x * sx, y * sy, w * sx, h * sy, 0, 0, w, h);
    sctx.globalCompositeOperation = "multiply";
    sctx.fillStyle = color;
    sctx.fillRect(0, 0, w, h);
    sctx.globalCompositeOperation = "destination-in";
    sctx.drawImage(image, x * sx, y * sy, w * sx, h * sy, 0, 0, w, h);
    return scratch;
  }

  // The app's own rows slipped sideways (now and then one pulled from elsewhere), with
  // red and cyan fringes on a few bands.
  function drawStolenPixels(image, x, y, w, h, rand) {
    const sx = image.width / width;
    const sy = image.height / height;
    const maxShift = Math.min(90, w * 0.1);
    for (let top = y; top < y + h;) {
      const band = Math.min(y + h - top, 3 + Math.floor(rand() * 24));
      const shift = rand() < 0.6 ? (rand() * 2 - 1) * maxShift : 0;
      const from = rand() < 0.12 ? clamp(top + (rand() * 2 - 1) * 90, 0, height - band) : top;
      ctx.drawImage(image, x * sx, from * sy, w * sx, band * sy, x + shift, top, w, band);
      top += band;
    }
    ctx.globalCompositeOperation = "lighter";
    ctx.globalAlpha = 0.5;
    for (let index = 0; index < 4; index += 1) {
      const band = Math.min(h, 6 + Math.floor(rand() * 36));
      const top = y + rand() * Math.max(0, h - band);
      const offset = 3 + rand() * 7;
      ctx.drawImage(tint(image, x, top, w, band, "#ff0000"), 0, 0, w, band, x + offset, top, w, band);
      ctx.drawImage(tint(image, x, top, w, band, "#00ffff"), 0, 0, w, band, x - offset, top, w, band);
    }
    ctx.globalCompositeOperation = "source-over";
    ctx.globalAlpha = 1;
  }

  // No snapshot to break: draw the corruption itself (a dark veil and neon tears).
  function drawFakeCorruption(x, y, w, h, rand) {
    ctx.fillStyle = "rgba(10, 6, 24, 0.35)";
    ctx.fillRect(x, y, w, h);
    for (let top = y; top < y + h;) {
      const band = 2 + Math.floor(rand() * 18);
      if (rand() < 0.45) {
        ctx.globalAlpha = 0.18 + rand() * 0.45;
        ctx.fillStyle = NEON[Math.floor(rand() * NEON.length)];
        ctx.fillRect(x + (rand() * 2 - 1) * w * 0.2, top, w * (0.3 + rand() * 0.7), band);
      }
      top += band;
    }
    ctx.globalAlpha = 1;
  }

  // Blocks copied from elsewhere in the window, smeared into big pixels, or stuck on a colour.
  function drawGlitchBlocks(image, x, y, w, h, rand) {
    const sx = image ? image.width / width : 1;
    const sy = image ? image.height / height : 1;
    // Bigger and more blocks on a bigger window.
    const unit = w * h > 400000 ? 16 : 8;
    const count = 4 + Math.floor(rand() * 9) + Math.min(24, Math.floor((w * h) / 50000));
    for (let index = 0; index < count; index += 1) {
      const bw = unit * (1 + Math.floor(rand() * 7));
      const bh = unit * (1 + Math.floor(rand() * 4));
      const bx = x + Math.floor(rand() * Math.max(1, (w - bw) / unit)) * unit;
      const by = y + Math.floor(rand() * Math.max(1, (h - bh) / unit)) * unit;
      const kind = rand();
      if (image && kind < 0.4) {
        const fromX = x + rand() * Math.max(1, w - bw);
        const fromY = y + rand() * Math.max(1, h - bh);
        ctx.drawImage(image, fromX * sx, fromY * sy, bw * sx, bh * sy, bx, by, bw, bh);
      } else if (image && kind < 0.7) {
        const cw = Math.max(1, Math.round(bw / 8));
        const ch = Math.max(1, Math.round(bh / 8));
        sctx.globalCompositeOperation = "copy";
        sctx.drawImage(image, bx * sx, by * sy, bw * sx, bh * sy, 0, 0, cw, ch);
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(scratch, 0, 0, cw, ch, bx, by, bw, bh);
        ctx.imageSmoothingEnabled = true;
      } else {
        ctx.globalAlpha = 0.55 + rand() * 0.4;
        ctx.fillStyle = NEON[Math.floor(rand() * NEON.length)];
        ctx.fillRect(bx, by, bw, bh);
        ctx.globalAlpha = 1;
      }
    }
  }

  function drawScanlines(x, y, w, h, rand) {
    ctx.fillStyle = "rgba(0, 0, 0, 0.16)";
    for (let line = y; line < y + h; line += 3) ctx.fillRect(x, line, w, 1);
    ctx.fillStyle = "rgba(220, 255, 255, 0.55)";
    for (let index = 0; index < 3; index += 1) ctx.fillRect(x, y + rand() * h, w, 1);
    const dots = clamp(Math.floor((w * h) / 1500), 90, 700);
    for (let index = 0; index < dots; index += 1) {
      ctx.fillStyle = rand() < 0.5 ? "rgba(255, 255, 255, 0.7)" : "rgba(0, 0, 0, 0.7)";
      const size = 1 + Math.floor(rand() * 2);
      ctx.fillRect(x + rand() * w, y + rand() * h, size, size);
    }
  }

  // Her tag on the broken patch, with a colour split.
  function drawHijackLabel(x, y, w, h, rand) {
    const size = Math.round(clamp(w / 26, 12, 40));
    ctx.font = `bold ${size}px ui-monospace, Menlo, Consolas, monospace`;
    ctx.textBaseline = "middle";
    const pad = size * 0.7;
    const tw = ctx.measureText(HIJACK_LABEL).width + pad * 2;
    const th = size * 1.8;
    const jitter = size * 0.4;
    const lx = x + clamp(w / 2 - tw / 2 + (rand() * 2 - 1) * jitter, 4, Math.max(4, w - tw - 4));
    const ly = y + clamp(h * 0.4 - th / 2, 8, Math.max(8, h - th - 8));
    const split = Math.max(1.5, size / 8);
    ctx.fillStyle = "rgba(8, 2, 20, 0.86)";
    ctx.fillRect(lx, ly, tw, th);
    ctx.fillStyle = "#28f0ff";
    ctx.fillText(HIJACK_LABEL, lx + pad - split, ly + th / 2);
    ctx.fillStyle = "#ff2bd6";
    ctx.fillText(HIJACK_LABEL, lx + pad + split, ly + th / 2);
    ctx.fillStyle = "#ffffff";
    ctx.fillText(HIJACK_LABEL, lx + pad, ly + th / 2);
  }

  function drawHijack(fx, progress, now) {
    // A new glitch pattern every step, so it jumps like a broken screen instead of flowing.
    const rand = random(fx.hijack.seed + Math.floor((now - fx.started) / HIJACK_STEP_MS) * 7919);
    // It starts at her hand, spreads over the whole window, glitches, then collapses to a
    // line and is gone.
    const grow = easeOut(clamp(progress / 0.22, 0, 1));
    const off = clamp((progress - 0.84) / 0.16, 0, 1);
    const seedW = Math.min(width, 120);
    const seedH = Math.min(height, 84);
    const w = seedW + (width - seedW) * grow;
    const h = Math.max(2, (seedH + (height - seedH) * grow) * (1 - off) ** 2);
    const x = clamp(fx.cx - w / 2 + (width / 2 - fx.cx) * grow, 0, Math.max(0, width - w));
    const y = clamp(fx.cy - h / 2 + (height / 2 - fx.cy) * grow, 0, Math.max(0, height - h));
    if (off === 0 && progress > 0.18 && rand() < 0.08) return; // a blink
    // Only a snapshot taken for this prank: an older one would show some other window.
    const image = snapshot && snapshotAt > fx.started - 8000 ? snapshot : null;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, y, w, h);
    ctx.clip();
    if (image) drawStolenPixels(image, x, y, w, h, rand);
    else drawFakeCorruption(x, y, w, h, rand);
    drawGlitchBlocks(image, x, y, w, h, rand);
    drawScanlines(x, y, w, h, rand);
    if (off === 0 && progress > 0.25 && progress < 0.8 && rand() < 0.7) drawHijackLabel(x, y, w, h, rand);
    ctx.restore();
    if (off > 0) {
      ctx.save();
      ctx.globalAlpha = 1 - off;
      ctx.fillStyle = "#e9fdff";
      ctx.shadowColor = "#7af2ff";
      ctx.shadowBlur = 16;
      ctx.fillRect(x, y + h / 2 - 1, w, 2);
      ctx.restore();
    }
  }

  // ---- Reaction effects (labelled in overlay/REACTIONS.md) -----------------
  // Friendlier, warm-palette animations for her non-prank reactions. Same contract:
  // one draw(fx, progress) per frame, progress 0..1 over the effect's duration, drawn
  // only on this transparent click-through layer.

  function roundRect(x, y, w, h, r) {
    const rr = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rr, y);
    ctx.arcTo(x + w, y, x + w, y + h, rr);
    ctx.arcTo(x + w, y + h, x, y + h, rr);
    ctx.arcTo(x, y + h, x, y, rr);
    ctx.arcTo(x, y, x + w, y, rr);
    ctx.closePath();
  }

  function heartPath(cx, cy, s) {
    ctx.beginPath();
    ctx.moveTo(cx, cy + s * 0.9);
    ctx.bezierCurveTo(cx - s * 1.3, cy - s * 0.2, cx - s * 0.55, cy - s * 1.0, cx, cy - s * 0.3);
    ctx.bezierCurveTo(cx + s * 0.55, cy - s * 1.0, cx + s * 1.3, cy - s * 0.2, cx, cy + s * 0.9);
    ctx.closePath();
  }

  // A — new email: an envelope pops in, its flap lifts, the letter rises with a "1" badge.
  function makeMail(cx, cy) {
    const rand = random(Math.round(cx * 17 + cy * 41 + width));
    const sparks = [];
    for (let i = 0; i < 9; i += 1) {
      sparks.push({ a: rand() * TAU, dist: 36 + rand() * 60,
        delay: 0.42 + rand() * 0.26, size: 1.6 + rand() * 2.6, warm: rand() < 0.5 });
    }
    return { sparks, w: clamp(Math.min(width, height) * 0.22, 120, 230) };
  }

  function drawMail(fx, progress) {
    const mail = fx.mail;
    const w = mail.w;
    const h = w * 0.64;
    const appear = easeOut(clamp(progress / 0.16, 0, 1));
    const pop = 1 + 0.12 * Math.sin(clamp(progress / 0.16, 0, 1) * Math.PI);
    const fade = clamp((1 - progress) / 0.18, 0, 1);
    const flap = easeOut(clamp((progress - 0.2) / 0.26, 0, 1));
    const letter = easeOut(clamp((progress - 0.4) / 0.34, 0, 1));
    const badge = clamp((progress - 0.52) / 0.16, 0, 1);
    ctx.save();
    ctx.globalAlpha = fade;
    ctx.translate(fx.cx, fx.cy);
    ctx.scale(appear * pop, appear * pop);

    const glow = ctx.createRadialGradient(0, 0, 0, 0, 0, w);
    glow.addColorStop(0, "rgba(255,243,210,0.55)");
    glow.addColorStop(1, "rgba(255,243,210,0)");
    ctx.fillStyle = glow;
    ctx.beginPath(); ctx.arc(0, 0, w, 0, TAU); ctx.fill();

    if (letter > 0) {
      ctx.save();
      ctx.translate(0, -h * 0.1 - letter * h * 0.6);
      roundRect(-w * 0.38, -h * 0.4, w * 0.76, h * 0.8, 5);
      ctx.fillStyle = "#fffdf6"; ctx.fill();
      ctx.strokeStyle = "rgba(47,93,100,0.3)"; ctx.lineWidth = 1.4; ctx.stroke();
      ctx.fillStyle = "rgba(47,93,100,0.32)";
      for (let i = 0; i < 3; i += 1) {
        ctx.fillRect(-w * 0.26, -h * 0.26 + i * h * 0.2, (i === 2 ? 0.46 : 0.62) * w, h * 0.055);
      }
      ctx.restore();
    }

    roundRect(-w / 2, -h / 2, w, h, 8);
    ctx.fillStyle = "#ece1c8"; ctx.fill();
    ctx.lineWidth = 2; ctx.strokeStyle = "#2f5d64"; ctx.stroke();

    ctx.beginPath();
    ctx.moveTo(-w / 2, h / 2); ctx.lineTo(-w * 0.04, -h * 0.02);
    ctx.moveTo(w / 2, h / 2); ctx.lineTo(w * 0.04, -h * 0.02);
    ctx.strokeStyle = "rgba(47,93,100,0.35)"; ctx.lineWidth = 1.3; ctx.stroke();

    const apexY = -h / 2 + h * 0.62 * (1 - flap);
    ctx.beginPath();
    ctx.moveTo(-w / 2, -h / 2); ctx.lineTo(w / 2, -h / 2); ctx.lineTo(0, apexY); ctx.closePath();
    ctx.fillStyle = flap < 0.5 ? "#e4d8ba" : "#d6c8a2";
    ctx.fill(); ctx.strokeStyle = "#2f5d64"; ctx.lineWidth = 2; ctx.stroke();

    if (badge > 0) {
      const bs = (0.5 + 0.5 * easeOut(badge)) * w * 0.2;
      const bpop = 1 + 0.3 * Math.sin(badge * Math.PI);
      ctx.save();
      ctx.translate(w * 0.42, -h * 0.46);
      ctx.scale(bpop, bpop);
      ctx.beginPath(); ctx.arc(0, 0, bs, 0, TAU);
      ctx.fillStyle = "#ff5a6a"; ctx.fill();
      ctx.strokeStyle = "#fff"; ctx.lineWidth = 2; ctx.stroke();
      ctx.fillStyle = "#fff";
      ctx.font = `700 ${Math.round(bs * 1.25)}px system-ui, sans-serif`;
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText("1", 0, bs * 0.08);
      ctx.restore();
    }

    for (const s of mail.sparks) {
      const t = clamp((progress - s.delay) / 0.34, 0, 1);
      if (t <= 0 || t >= 1) continue;
      ctx.globalAlpha = fade * (1 - t);
      ctx.fillStyle = s.warm ? "#ffd76a" : "#7fd1c4";
      ctx.beginPath();
      ctx.arc(Math.cos(s.a) * s.dist * easeOut(t), Math.sin(s.a) * s.dist * easeOut(t) - h * 0.1,
              s.size * (1 - t * 0.5), 0, TAU);
      ctx.fill();
    }
    ctx.restore();
  }

  // B / C — new message or reminder: a notification chip pops with sound-wave rings.
  function drawPing(fx, progress) {
    const base = clamp(Math.min(width, height) * 0.2, 90, 200);
    const fade = clamp((1 - progress) / 0.2, 0, 1);
    ctx.save();
    ctx.globalAlpha = fade;
    for (let i = 0; i < 3; i += 1) {
      const ph = clamp(progress * 1.3 - i * 0.16, 0, 1);
      if (ph <= 0 || ph >= 1) continue;
      ctx.beginPath();
      ctx.arc(fx.cx, fx.cy, base * (0.5 + easeOut(ph) * 1.1), 0, TAU);
      ctx.strokeStyle = `rgba(127,209,196,${(1 - ph) * 0.8})`;
      ctx.lineWidth = 2 + (1 - ph) * 5;
      ctx.stroke();
    }
    const grow = easeOut(clamp(progress / 0.2, 0, 1));
    const shrink = progress > 0.82 ? clamp((1 - progress) / 0.18, 0, 1) : 1;
    const over = 1 + 0.18 * Math.sin(clamp(progress / 0.2, 0, 1) * Math.PI);
    const r = base * 0.5 * grow * shrink * over;
    if (r > 1) {
      ctx.save();
      ctx.translate(fx.cx, fx.cy);
      const grad = ctx.createLinearGradient(0, -r, 0, r);
      grad.addColorStop(0, "#5fe0d0");
      grad.addColorStop(1, "#2f9d8f");
      roundRect(-r, -r * 0.8, r * 2, r * 1.6, r * 0.4);
      ctx.fillStyle = grad; ctx.fill();
      ctx.beginPath();
      ctx.moveTo(-r * 0.35, r * 0.75); ctx.lineTo(-r * 0.05, r * 0.75); ctx.lineTo(-r * 0.5, r * 1.15);
      ctx.closePath(); ctx.fill();
      ctx.strokeStyle = "rgba(255,255,255,0.9)"; ctx.lineWidth = 2;
      roundRect(-r, -r * 0.8, r * 2, r * 1.6, r * 0.4); ctx.stroke();
      ctx.fillStyle = "#fffdf7";
      for (let i = -1; i <= 1; i += 1) {
        const dot = clamp((progress - 0.24 - (i + 1) * 0.07) / 0.1, 0, 1);
        ctx.globalAlpha = fade * dot;
        ctx.beginPath(); ctx.arc(i * r * 0.44, 0, r * 0.14, 0, TAU); ctx.fill();
      }
      ctx.restore();
    }
    ctx.restore();
  }

  // D — affection / greeting: hearts drift up and fade.
  function makeHearts(cx, cy) {
    const rand = random(Math.round(cx * 29 + cy * 53 + width));
    const hearts = [];
    for (let i = 0; i < 7; i += 1) {
      hearts.push({ x: (rand() - 0.5) * 80, delay: i * 0.08 + rand() * 0.05,
        rise: 120 + rand() * 90, sway: (rand() - 0.5) * 42, size: 10 + rand() * 12,
        hue: ["#ff8fb0", "#ff6f91", "#ffa7c4"][Math.floor(rand() * 3)] });
    }
    return hearts;
  }

  function drawHearts(fx, progress) {
    ctx.save();
    for (const hr of fx.hearts) {
      const t = clamp((progress - hr.delay) / (1 - hr.delay), 0, 1);
      if (t <= 0) continue;
      const y = fx.cy - easeOut(t) * hr.rise;
      const x = fx.cx + hr.x + Math.sin(t * 6 + hr.delay * 10) * hr.sway;
      const scale = t < 0.2 ? easeOut(t / 0.2) : 1;
      ctx.globalAlpha = t > 0.7 ? (1 - t) / 0.3 : 1;
      ctx.fillStyle = hr.hue;
      ctx.shadowColor = "rgba(255,120,160,0.6)";
      ctx.shadowBlur = 8;
      heartPath(x, y, hr.size * scale);
      ctx.fill();
    }
    ctx.restore();
  }

  function tick(now) {
    if (!active) return;
    const progress = clamp((now - active.started) / active.duration, 0, 1);
    ctx.clearRect(0, 0, width, height);
    if (active.kind === "crack") drawCrack(active, progress);
    else if (active.kind === "portal") drawPortal(active, progress, now);
    else if (active.kind === "text_shatter") drawTextShatter(active, progress);
    else if (active.kind === "ripple") drawRipple(active, progress);
    else if (active.kind === "pixel_dissolve") drawPixelDissolve(active, progress);
    else if (active.kind === "lightning") drawLightning(active, progress);
    else if (active.kind === "tear") drawTear(active, progress);
    else if (active.kind === "collapse") drawCollapse(active, progress);
    else if (active.kind === "hijack") drawHijack(active, progress, now);
    else if (active.kind === "mail") drawMail(active, progress);
    else if (active.kind === "ping") drawPing(active, progress);
    else if (active.kind === "hearts") drawHearts(active, progress);
    if (progress < 1) animationFrame = requestAnimationFrame(tick);
    else animationFrame = 0;
  }

  function stop() {
    if (animationFrame) cancelAnimationFrame(animationFrame);
    animationFrame = 0;
    active = null;
    ctx.clearRect(0, 0, width, height);
  }

  function start(kind, config) {
    stop();
    resize();
    const requestedX = Number(config.origin_x);
    const requestedY = Number(config.origin_y);
    let cx = clamp(Number.isFinite(requestedX) ? requestedX : 0.5, 0.05, 0.95) * width;
    let cy = clamp(Number.isFinite(requestedY) ? requestedY : 0.35, 0.05, 0.95) * height;
    const radius = clamp(Math.min(width, height) * 0.22, 64, 230);
    if (kind === "portal") {
      cx = width > radius * 2.4 ? clamp(cx, radius * 1.2, width - radius * 1.2) : width / 2;
      cy = height > radius * 2.4 ? clamp(cy, radius * 1.2, height - radius * 1.2) : height / 2;
    }
    active = {
      kind, cx, cy, radius,
      paths: kind === "crack" || kind === "collapse" ? makeCracks(cx, cy) : null,
      tiles: kind === "collapse" ? makeTiles(Math.floor(cx * 31 + cy * 17)) : null,
      glyphs: kind === "text_shatter" ? makeGlyphs(cx, cy, config.title) : null,
      pixels: kind === "pixel_dissolve" ? makePixels(cx, cy) : null,
      bolts: kind === "lightning" ? makeLightning(cx, cy) : null,
      tear: kind === "tear" ? makeTear(cx, cy) : null,
      hijack: kind === "hijack" ? makeHijack(cx, cy) : null,
      mail: kind === "mail" ? makeMail(cx, cy) : null,
      hearts: kind === "hearts" ? makeHearts(cx, cy) : null,
      title: config.title,
      started: performance.now(),
      duration: config.duration,
    };
    animationFrame = requestAnimationFrame(tick);
  }

  window.addEventListener("resize", () => {
    if (!active) return;
    const kind = active.kind;
    const origin_x = active.cx / width;
    const origin_y = active.cy / height;
    const duration = Math.max(100, active.duration - (performance.now() - active.started));
    start(kind, { origin_x, origin_y, duration, title: active.title });
  });

  window.HermesEffectCanvas = { start, stop, setSnapshot };
})();
