// Shared by the desktop overlay and sprite review page. No dependencies.
class HermesSpritePlayer {
  constructor(canvas, assetRoot) {
    this.canvas = canvas;
    this.context = canvas.getContext("2d");
    this.assetRoot = new URL(assetRoot, document.baseURI);
    this.images = {};
    this.clip = "core/idle";
    this.index = 0;
    this.elapsed = 0;
    this.paused = false;
    this.holding = false;
    this.loopOverride = undefined;
    this.ready = this.load();
  }

  async load() {
    const response = await fetch(new URL("manifest.json", this.assetRoot));
    if (!response.ok) throw new Error(`Sprite manifest: HTTP ${response.status}`);
    this.manifest = await response.json();
    await Promise.all(Object.entries(this.manifest.atlases).map(async ([name, path]) => {
      const image = new Image();
      image.src = new URL(path, this.assetRoot).href;
      await image.decode();
      this.images[name] = image;
    }));

    [this.canvas.width, this.canvas.height] = this.manifest.cell;
    const [displayWidth, displayHeight] = this.manifest.display || this.manifest.cell;
    this.canvas.style.width = `${displayWidth}px`;
    this.canvas.style.height = `${displayHeight}px`;

    this.play(this.clip);
    this.lastTime = performance.now();
    this.tick = this.tick.bind(this);
    this.animationFrame = requestAnimationFrame(this.tick);
    return this;
  }

  play(name, options = {}) {
    if (this.manifest && !this.manifest.clips[name]) return false;
    this.clip = name;
    this.loopOverride = typeof options.loop === "boolean" ? options.loop : undefined;
    this.index = 0;
    this.elapsed = 0;
    this.holding = false;
    if (this.manifest) this.lastTime = performance.now();
    this.draw();
    return true;
  }

  seek(index) {
    const clip = this.manifest?.clips[this.clip];
    if (!clip) return;
    this.index = Math.max(0, Math.min(clip.frames.length - 1, index));
    this.elapsed = 0;
    this.holding = false;
    this.draw();
  }

  draw() {
    const frame = this.manifest?.clips[this.clip]?.frames[this.index];
    if (!frame || !this.images[frame.atlas]) return;
    this.context.clearRect(0, 0, this.canvas.width, this.canvas.height);
    this.context.drawImage(this.images[frame.atlas], ...frame.rect, 0, 0, ...this.manifest.cell);
    this.canvas.dispatchEvent(new CustomEvent("spriteframe", {
      detail: { clip: this.clip, index: this.index, contact: frame.event },
    }));
  }

  completeClip(clipName, clip, wasOneCycleOverride) {
    const detail = {
      clip: clipName,
      index: clip.frames.length - 1,
      after: clip.after,
      loopOverridden: wasOneCycleOverride,
    };

    this.loopOverride = undefined;
    if (clip.after === "hold_last") {
      this.index = clip.frames.length - 1;
      this.elapsed = 0;
      this.holding = true;
    } else {
      const repeatAfter = clip.after === "repeat" && !wasOneCycleOverride;
      const nextClip = repeatAfter
        ? clipName
        : (this.manifest.clips[clip.after] ? clip.after : "core/idle");
      this.clip = nextClip;
      this.index = 0;
      this.holding = false;
      this.draw();
    }

    this.canvas.dispatchEvent(new CustomEvent("spritecomplete", { detail }));
  }

  tick(now) {
    const delta = Math.max(0, now - this.lastTime);
    this.lastTime = now;

    if (!this.paused && !this.holding) {
      this.elapsed += delta;
      while (!this.holding) {
        const clipName = this.clip;
        const clip = this.manifest.clips[clipName];
        const frame = clip.frames[this.index];
        const duration = Math.max(1, Number(frame.duration) || 1);
        if (this.elapsed < duration) break;

        this.elapsed -= duration;
        this.index += 1;
        if (this.index < clip.frames.length) {
          this.draw();
          continue;
        }

        const wasOneCycleOverride = this.loopOverride === false && clip.loop;
        if (this.loopOverride ?? clip.loop) {
          this.index = 0;
          this.draw();
          continue;
        }

        this.completeClip(clipName, clip, wasOneCycleOverride);
      }
    }

    this.animationFrame = requestAnimationFrame(this.tick);
  }

  destroy() {
    cancelAnimationFrame(this.animationFrame);
  }
}

window.HermesSpritePlayer = HermesSpritePlayer;
