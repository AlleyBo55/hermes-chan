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
    this.play(this.clip);
    this.lastTime = performance.now();
    this.tick = this.tick.bind(this);
    this.animationFrame = requestAnimationFrame(this.tick);
    return this;
  }

  play(name, options = {}) {
    if (this.manifest && !this.manifest.clips[name]) return false;
    this.clip = name;
    this.loopOverride = options.loop;
    this.index = 0;
    this.elapsed = 0;
    this.draw();
    return true;
  }

  seek(index) {
    const clip = this.manifest?.clips[this.clip];
    if (!clip) return;
    this.index = Math.max(0, Math.min(clip.frames.length - 1, index));
    this.elapsed = 0;
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

  tick(now) {
    const delta = Math.min(now - this.lastTime, 250);
    this.lastTime = now;
    if (!this.paused) {
      let clip = this.manifest.clips[this.clip];
      this.elapsed += delta;
      while (this.elapsed >= clip.frames[this.index].duration) {
        this.elapsed -= clip.frames[this.index].duration;
        this.index += 1;
        if (this.index >= clip.frames.length) {
          if (this.loopOverride ?? clip.loop) {
            this.index = 0;
          } else if (clip.after === "hold_last") {
            this.index = clip.frames.length - 1;
            this.elapsed = 0;
            break;
          } else {
            this.clip = this.manifest.clips[clip.after] ? clip.after : "core/idle";
            this.loopOverride = undefined;
            this.index = 0;
            clip = this.manifest.clips[this.clip];
          }
        }
        this.draw();
      }
    }
    this.animationFrame = requestAnimationFrame(this.tick);
  }

  destroy() { cancelAnimationFrame(this.animationFrame); }
}

window.HermesSpritePlayer = HermesSpritePlayer;
