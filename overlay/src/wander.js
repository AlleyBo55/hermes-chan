// Hermes-chan roams the desktop while she is idle and unpinned, after the behaviour
// model of the DORAEMON-AI shimeji engine. Every visible window's top edge is a ledge
// she can stand on and ride; she walks, jumps between ledges and the floor, climbs
// screen edges and window sides, reacts to the window you are using, and now and then
// plays a harmless visual prank on a window. The overlay is a small native window, so
// moving her moves that window.
(() => {
  // Matched to the v2 cycles so her feet don't slide: a 22 px step twice per 720 ms walk
  // cycle, and two hand-over-hand pulls per 880 ms climb cycle.
  const WALK_SPEED = 60; // logical px per second
  const CLIMB_SPEED = 70;
  const GRAVITY = 2000; // px per second squared
  const MAX_FALL_SPEED = 1400;
  // Hops: about 5% of the screen up or down, then she stands on that height as if on a
  // step and walks along it. Never above the ceiling (head on screen) or below the floor.
  const HOP_STEP = 0.05;
  const HOP_REACH = [15, 80]; // sideways distance of a hop
  const HOP_ARC = [18, 36]; // how far above the higher end of a hop its apex sits
  const HOP_UP_CHANCE = 0.6;
  const CROUCH_S = 0.16; // crouch before take-off
  const FEET_PADDING = 5; // transparent rows under her shoes (11 of 208 cell px, shown at half size)
  // Visible sprite edge that touches the wall while climbing, in display px from the canvas left.
  const CLIMB_REACH = { leftSide: 76, rightSide: 20 };
  const LEDGE_MARGIN = 30; // keep her centre this far inside a ledge's ends
  const MIN_LEDGE_WIDTH = 120;
  const CATCH_RANGE = 24; // a drop this close to a window top lands on it
  const EDGE_ROOM = 40;
  const WINDOW_POLL_MS = 1000;
  const START_DELAY = [1500, 3500];
  const YIELD_COOLDOWN = 4000;
  const TRIP_CHANCE = 0.2; // per beat on the floor: go do something with the active window
  const FOCUS_TRIP_CHANCE = 0.8; // when you focus another window
  const FOCUS_TRIP_COOLDOWN = 6000;
  const WALK_OFF_CHANCE = 0.3; // per walk on a window top: hop off the end
  const WATCH_CHANCE = 0.8; // per beat while a video is in front: keep watching, else turn round to react
  const CLING_MS = [1500, 5000]; // how long she hangs on a wall or window side between pulls
  const CLIMB_ON_CHANCE = 0.45; // after clinging: climb higher, else let go and fall
  const WINDOW_CLING_CHANCE = 0.4; // window climbs that pause partway up
  const CLIMB_SHARE = 0.5; // trips to the active window that become a climb up its side
  const MISCHIEF_COOLDOWN = 12000; // at least this long between pranks
  const MISCHIEF = [
    { effect: "bend", clip: "action/grab_window_bend" },
    { effect: "glitch", clip: "action/glitch" },
    ...["crack", "portal", "text_shatter", "ripple", "pixel_dissolve",
      "lightning", "tear"].map((effect) => ({ effect, clip: `action/${effect}` })),
  ];
  // Real pranks on the active window. She grabs it on her grip frame (a fling lets go on
  // release), and the overlay puts the window back a moment later. Only when she stands
  // next to that window, and the overlay refuses while you are using the computer.
  const GRABS = [
    { grab: "shake", clip: "action/grab_window_shake", event: "grip" },
    { grab: "nudge", clip: "action/grab_window_drag", event: "grip" },
    { grab: "throw", clip: "action/grab_window_throw", event: "release" },
    { grab: "pull_down", clip: "action/grab_window_drag", event: "grip" },
  ];
  // How long each effect runs once it fires (matches the overlay's own timings), and how
  // long a moved window stays moved. She holds still until it is over, so nothing she does
  // next (a hop uses her falling pose) plays on top of a prank that is still running.
  const EFFECT_MS = {
    bend: 1100, glitch: 1200, crack: 1400, portal: 2200, text_shatter: 1650, ripple: 1400,
    pixel_dissolve: 1600, lightning: 1300, tear: 1700, collapse: 2600, hijack: 2000,
    shake: 1500, nudge: 2900, throw: 2900, pull_down: 3000,
  };
  // These come out of her hand, so they get a patch of screen centred on it; the rest act
  // on the window under her hand (bend, glitch, crack, broken text, tear, collapse).
  const FROM_HAND = new Set(["portal", "lightning", "ripple", "pixel_dissolve"]);
  const GRAB_SHARE = 0.4; // pranks next to the active window that grab it for real
  // The fake "broken app": the window under her hand cracks and falls apart, then is fine.
  const BREAK_UI = { effect: "collapse", clip: "action/crack", event: "effect_start", snapshot: true };
  // "She hijacked your app": the window around her hand glitches out with its own pixels.
  const HIJACK = { effect: "hijack", clip: "action/glitch", event: "effect_start", snapshot: true };
  const BREAK_SHARE = 0.5; // pranks on a window that break it (collapse or hijack, half each)
  // What she does once she reaches the window, all facing it with her back to you.
  const LOOKS = {
    watch: { clip: "back/idle", ms: [8000, 16000] },
    work: { clip: "back/working", ms: [5000, 10000] },
    peek: { clip: "back/peek", ms: [3000, 5000] },
    point: { clip: "back/point", ms: [2500, 4000] },
    prank: { clip: "action/mischief", ms: [2600, 3600] },
  };
  // What the active window is for, read from its app name and title.
  // App names differ per OS: "Code" (VS Code), "WindowsTerminal", "gnome-terminal-server".
  const ACTIVITIES = [
    { kind: "video", pattern: /youtube|netflix|twitch|prime video|disney\+|vimeo|bilibili|crunchyroll|movies & tv|films & tv|media player|\b(iina|vlc|quicktime player|infuse|mpv|movist|tv|mpc-hc|mpc-be|potplayer|totem|celluloid|smplayer|haruna)\b/i },
    { kind: "code", pattern: /visual studio code|sublime text|android studio|\b(kiro|code|cursor|windsurf|xcode|zed|intellij idea|pycharm|webstorm|goland|clion|rider|rustrover|terminal|iterm2?|warp|ghostty|kitty|wezterm|alacritty|windowsterminal|powershell|pwsh|konsole|gnome-terminal(-server)?|xterm|tilix)\b/i },
  ];
  const BEATS = [
    { kind: "walk", weight: 28, ms: [3000, 7000] },
    { kind: "stand", weight: 16, ms: [1500, 3500] },
    { kind: "sit", weight: 9, ms: [4000, 8000] },
    { kind: "emote", weight: 12, ms: [1500, 3000] },
    { kind: "hop", weight: 45 }, // hop a step up or down and stand there
    { kind: "wall", weight: 8 }, // climb a screen edge
    { kind: "mischief", weight: 22 }, // a prank on a nearby window
  ];
  const EMOTES = ["happy", "proud", "shy", "love", "surprised"];

  const between = ([low, high]) => low + Math.random() * (high - low);
  const pickWeighted = (items) => {
    let roll = Math.random() * items.reduce((sum, item) => sum + item.weight, 0);
    return items.find((item) => (roll -= item.weight) <= 0) || items[0];
  };
  const toFrame = (data) => {
    const frame = {
      app: String(data?.app || ""),
      title: String(data?.title || ""),
      x: Number(data?.x),
      y: Number(data?.y),
      w: Number(data?.w),
      h: Number(data?.h),
    };
    const valid = [frame.x, frame.y, frame.w, frame.h].every(Number.isFinite) && frame.w > 0 && frame.h > 0;
    return valid ? frame : null;
  };
  // A tab switch renames the window without moving it; a drag moves it without renaming it.
  const sameWindow = (a, b) => Boolean(a && b && a.app === b.app
    && (a.title === b.title || (a.x === b.x && a.y === b.y)));
  const activityOf = (frame) => {
    if (!frame) return null;
    const text = `${frame.app} ${frame.title}`;
    return ACTIVITIES.find((activity) => activity.pattern.test(text))?.kind || null;
  };

  class HermesWander {
    constructor({ sprites, chara, canRoam, canFall, canMischief, invoke, mood, onRelease }) {
      const tauri = window.__TAURI__ || {};
      this.win = tauri.window?.getCurrentWindow?.() || null;
      // A module function in Tauri v2, not a Window method.
      this.currentMonitor = tauri.window?.currentMonitor || null;
      this.Position = tauri.dpi?.LogicalPosition || tauri.window?.LogicalPosition || null;
      this.sprites = sprites;
      this.chara = chara;
      this.canRoam = canRoam;
      this.canFall = canFall;
      this.canMischief = canMischief || (() => false);
      this.invoke = invoke;
      this.mood = mood;
      this.onRelease = onRelease;
      this.phase = null; // null | drop | rest | walk | approach | climb | cling | jump | fall | land
      this.surface = null; // null (unknown) | "floor" | "ledge"
      this.support = null; // the ledge she stands on: { win, y, left, right }
      this.active = null; // the active window's frame, from its app and title
      this.windows = [];
      this.ledgeCache = null;
      this.nextPoll = 0;
      this.polling = false;
      this.trip = null;
      this.jump = null;
      this.tripPending = false;
      this.lastTripAt = 0;
      this.lastMischiefAt = 0;
      this.walkOff = false;
      this.looking = null;
      this.prank = null;
      this.lineY = null; // the height she stands at between floor and ceiling
      this.screen = null;
      this.clip = null;
      this.dir = Math.random() < 0.5 ? -1 : 1;
      this.freeAt = 0;
      this.cooldownUntil = 0;
      this.measuring = false;
      this.pending = null;
      this.sending = false;
      this.last = performance.now();

      chara.addEventListener("spritecomplete", (e) => {
        if (this.phase === "land" && e.detail?.clip === "movement/land") this.release(true);
        // A one-shot look such as back/point hands over to its follow-up clip; keep owning it.
        else if (this.phase === "rest" && e.detail?.clip === this.clip) this.clip = this.sprites.clip;
      });
      chara.addEventListener("spriteframe", (e) => {
        const pending = this.prank;
        // Only while she is still casting this prank, on its effect frame.
        if (!pending || this.looking !== "prank" || e.detail?.clip !== pending.clip
          || e.detail?.contact !== pending.event) return;
        this.prank = null;
        if (pending.grab) {
          this.invoke("self_grab", { gesture: pending.grab, direction: this.dir }).catch(() => {
            // Refused while you are using the computer: bend the window instead of moving it.
            const hand = this.handPoint();
            this.invoke("play_effect", { effect: "bend", ...pending.frame, handX: hand.x, handY: hand.y })
              .catch(() => {});
          });
          return;
        }
        const hand = this.handPoint();
        this.invoke("play_effect", { effect: pending.effect, x: pending.frame.x, y: pending.frame.y,
          w: pending.frame.w, h: pending.frame.h, handX: hand.x, handY: hand.y }).catch(() => {});
      });
      this.tick = this.tick.bind(this);
      if (this.available) requestAnimationFrame(this.tick);
    }

    get available() {
      return Boolean(this.win && this.Position && this.currentMonitor);
    }

    // True while roaming owns her animation; the state renderer waits for it.
    get busy() {
      return this.phase !== null;
    }

    // True while she stands facing a video with you; main.js picks video reactions then.
    get watchingVideo() {
      return this.phase === "rest" && this.looking === "watch";
    }

    // Fed from the overlay's active-window events: what you are using decides her trips.
    setActiveWindow(data) {
      const next = toFrame(data);
      const previous = this.active;
      this.active = next;
      if (!this.screen || this.surface === null || !this.canFall()) return;
      if (sameWindow(previous, next)) {
        // Same window, new page (a video started or ended): rethink now.
        if (activityOf(previous) !== activityOf(next) && this.settled()) this.beatEnd = 0;
        return;
      }
      if (this.trip?.active) this.trip = null;
      const video = activityOf(next) === "video";
      if (next && (video || (performance.now() - this.lastTripAt > FOCUS_TRIP_COOLDOWN
          && Math.random() < FOCUS_TRIP_CHANCE))) {
        this.tripPending = true;
        if (this.settled()) this.beatEnd = 0;
      }
    }

    settled() {
      return this.phase === "rest" || this.phase === "walk";
    }

    tick(now) {
      const dt = Math.min(0.05, Math.max(0, (now - this.last) / 1000));
      this.last = now;
      try {
        this.pollWindows(now);
        this.step(now, dt);
      } catch (error) {
        console.warn("Hermes-chan roaming skipped a beat", error);
        this.release(true);
      }
      requestAnimationFrame(this.tick);
    }

    // The window list gives her ledges everywhere; a window that moves carries her along.
    pollWindows(now) {
      if (this.polling || now < this.nextPoll || !this.invoke) return;
      this.polling = true;
      this.nextPoll = now + WINDOW_POLL_MS;
      this.invoke("list_windows")
        .then((list) => this.updateWindows(Array.isArray(list) ? list : []))
        .catch(() => {})
        .finally(() => {
          this.polling = false;
        });
    }

    updateWindows(list) {
      this.windows = list;
      this.ledgeCache = null;
      const support = this.support;
      if (!support || !this.screen || this.surface === null || !this.canFall()) return;
      const next = list.find((win) => win.id === support.win.id);
      const riding = this.surface === "ledge" && ["rest", "walk", "approach", null].includes(this.phase);
      if (next && riding && (next.x !== support.win.x || next.y !== support.win.y)) {
        this.x += next.x - support.win.x;
        this.y += next.y - support.win.y;
        if (this.trip) this.trip.targetX += next.x - support.win.x;
        this.moveTo();
      }
      if (this.surface !== "ledge") return;
      this.support = next ? this.ledgeUnder(this.x, next.id) : null;
      if (!this.support) this.loseFooting(); // closed, hidden, or something now covers her spot
    }

    step(now, dt) {
      if (!this.canFall()) this.surface = null; // something else may move her; re-measure later
      if (this.phase === "drop") return;
      if (this.phase === "fall" || this.phase === "land" || this.phase === "jump") {
        if (!this.canFall()) {
          this.pending = null;
          this.release(false);
        } else if (this.phase === "fall") {
          this.fall(dt);
        } else if (this.phase === "jump") {
          this.jumpStep(dt);
        } else if (this.sprites.clip !== "movement/land") {
          this.release(false);
        }
        return;
      }
      if (!this.canRoam()) {
        this.freeAt = 0;
        if (this.phase) this.release(true);
        return;
      }
      if (this.phase && this.sprites.clip !== this.clip) {
        // Something else took over her animation (an emote, a gesture): let it play.
        this.cooldownUntil = now + YIELD_COOLDOWN;
        this.release(false);
        return;
      }
      if (!this.phase) {
        if (!this.freeAt) this.freeAt = now + between(START_DELAY);
        if (now >= this.freeAt && now >= this.cooldownUntil && !this.measuring) this.begin();
        return;
      }
      if (this.phase === "walk") this.walk(dt);
      else if (this.phase === "approach") this.approach(dt, now);
      else if (this.phase === "climb") this.climb(dt, now);
      else if (this.phase === "cling" && now >= this.beatEnd) this.afterCling();
      if (this.settled() && now >= this.beatEnd) this.nextBeat(now);
    }

    async begin() {
      this.measuring = true;
      try {
        if (!(await this.measure()) || !this.canRoam()) return;
        if (this.settle(true)) this.nextBeat(performance.now());
      } catch (error) {
        console.warn("Hermes-chan could not read the screen", error);
        this.cooldownUntil = performance.now() + 10000;
      } finally {
        this.measuring = false;
      }
    }

    // Called when the user lets go of her. Returns false when it cannot help,
    // so the caller keeps its old landing.
    drop(animate) {
      if (!this.available) return false;
      // Hold the dragged pose while the screen is measured.
      this.phase = "drop";
      this.clip = null;
      this.trip = null;
      this.surface = null;
      this.lineY = null;
      this.measure()
        .then((ok) => {
          if (this.phase !== "drop") return;
          if (!ok || !this.canFall()) this.release(true);
          else if (this.settle(animate)) this.land(animate);
        })
        .catch((error) => {
          console.warn("Hermes-chan could not read the screen", error);
          if (this.phase === "drop") this.release(true);
        });
      return true;
    }

    async measure() {
      const [position, scale, monitor] = await Promise.all([
        this.win.outerPosition(),
        this.win.scaleFactor(),
        this.currentMonitor(),
      ]);
      if (!monitor) return false;
      const monitorScale = monitor.scaleFactor || scale;
      const area = monitor.workArea || { position: monitor.position, size: monitor.size };
      const left = area.position.x / monitorScale;
      const top = area.position.y / monitorScale;
      const rect = this.chara.getBoundingClientRect();
      this.x = position.x / scale;
      this.y = position.y / scale;
      this.feet = rect.bottom - FEET_PADDING;
      this.canvasLeft = rect.left;
      this.canvasTop = rect.top;
      // The whole display, menu bar included: her window sits above the menu bar level.
      this.displayTop = monitor.position.y / monitorScale;
      this.width = window.innerWidth;
      this.half = this.width / 2;
      this.screen = {
        left,
        top,
        right: left + area.size.width / monitorScale,
        bottom: top + area.size.height / monitorScale,
      };
      this.ledgeCache = null;
      return true;
    }

    // Work out what she stands on, or start a fall. Returns true when she is grounded.
    settle(animate) {
      if (this.lineY != null && Math.abs(this.y - this.lineY) <= 2 && this.y < this.floorY() - 2) {
        this.surface = "line";
        this.y = this.lineY;
        this.moveTo();
        return true;
      }
      this.lineY = null;
      const ledge = this.ledgeNear(this.x, this.y);
      if (ledge) {
        this.surface = "ledge";
        this.support = ledge;
        this.y = ledge.y - this.feet;
        const [min, max] = this.range();
        this.x = Math.min(max, Math.max(min, this.x));
      } else if (this.y < this.floorY() - 2) {
        this.surface = "floor";
        this.support = null;
        this.startFall(animate);
        return false;
      } else {
        this.surface = "floor";
        this.support = null;
        this.y = this.floorY();
      }
      this.moveTo();
      return true;
    }

    floorY() {
      return this.screen.bottom - this.feet;
    }

    // Window x positions that keep the whole overlay, bubble and chat box included, on screen.
    screenRange() {
      return [this.screen.left, Math.max(this.screen.left, this.screen.right - this.width)];
    }

    // Where she can walk on her current surface.
    range(surface = this.surface, ledge = this.support) {
      const [min, max] = this.screenRange();
      if (surface !== "ledge" || !ledge) return [min, max];
      const low = Math.max(min, ledge.left + LEDGE_MARGIN - this.half);
      const high = Math.min(max, ledge.right - LEDGE_MARGIN - this.half);
      return high > low ? [low, high] : [min, max];
    }

    // The visible stretches of every window's top edge, front to back. A window in front
    // that crosses another's top edge hides that part of it. Edges with no room above
    // (a maximised window under the menu bar) are left out.
    ledges() {
      if (this.ledgeCache) return this.ledgeCache;
      const result = [];
      if (!this.screen || this.feet == null) return result;
      this.windows.forEach((win, index) => {
        if (win.y < this.displayTop + 12 || win.y > this.screen.bottom - 80) return;
        if (win.y - this.feet < this.highestCling()) return;
        let pieces = [[Math.max(win.x, this.screen.left), Math.min(win.x + win.w, this.screen.right)]];
        for (const front of this.windows.slice(0, index)) {
          if (front.y >= win.y || front.y + front.h <= win.y) continue;
          pieces = pieces.flatMap(([a, b]) => {
            const c = front.x;
            const d = front.x + front.w;
            if (d <= a || c >= b) return [[a, b]];
            return [[a, Math.min(b, c)], [Math.max(a, d), b]].filter(([p, q]) => q > p);
          });
        }
        for (const [left, right] of pieces) {
          if (right - left >= MIN_LEDGE_WIDTH) result.push({ win, y: win.y, left, right });
        }
      });
      this.ledgeCache = result;
      return result;
    }

    // The ledge under her centre at window x, optionally on one window only.
    ledgeUnder(x, windowId) {
      const centre = x + this.half;
      return this.ledges().find((ledge) => (windowId == null || ledge.win.id === windowId)
        && centre >= ledge.left + 8 && centre <= ledge.right - 8) || null;
    }

    // A ledge whose top is within reach of her feet at window position (x, y).
    ledgeNear(x, y) {
      const centre = x + this.half;
      return this.ledges().find((ledge) => centre >= ledge.left + 8 && centre <= ledge.right - 8
        && Math.abs(ledge.y - this.feet - y) <= CATCH_RANGE) || null;
    }

    nextBeat(now) {
      this.trip = null;
      this.looking = null;
      if (this.surface === "line" && activityOf(this.active) === "video") {
        // A video started: drop down to watch it with you.
        this.loseFooting();
        return;
      }
      const activity = this.surface === "floor" ? activityOf(this.active) : null;
      if (activity === "video") {
        // Watch with you: mostly stay facing the video, sometimes turn round to react.
        this.tripPending = false;
        if (Math.random() < WATCH_CHANCE) this.startTrip(now, activity);
        else this.restBeat(now, BEATS.find((beat) => beat.kind === "emote"));
        return;
      }
      if (this.surface === "floor" && this.active && (this.tripPending || Math.random() < TRIP_CHANCE)) {
        this.tripPending = false;
        if (this.startTrip(now, activity)) return;
      }
      this.tripPending = false;
      let beat = pickWeighted(BEATS);
      if (beat.kind === "hop" && this.startHop()) return;
      if (beat.kind === "wall" && this.startWallClimb()) return;
      if (beat.kind === "mischief" && this.startMischief(now)) return;
      if (!beat.ms) beat = BEATS.find((option) => option.kind === "stand");
      if (beat.kind !== "walk") {
        this.restBeat(now, beat);
        return;
      }
      this.beatEnd = now + between(beat.ms);
      const [min, max] = this.range();
      if (this.x - min < EDGE_ROOM) this.dir = 1;
      else if (max - this.x < EDGE_ROOM) this.dir = -1;
      else if (Math.random() < 0.5) this.dir = -this.dir;
      this.walkOff = this.surface === "ledge" && Math.random() < WALK_OFF_CHANCE;
      this.phase = "walk";
      this.show(this.walkClip());
    }

    restBeat(now, beat) {
      this.beatEnd = now + between(beat.ms);
      this.phase = "rest";
      if (beat.kind === "sit") {
        this.show("action/sit", { loop: false });
      } else if (beat.kind === "emote") {
        this.show(this.emotionClip(EMOTES[Math.floor(Math.random() * EMOTES.length)]) || "core/idle");
      } else {
        this.show(this.emotionClip(this.mood?.()) || "core/idle");
      }
    }

    // Head for the active window and do something with it: watch a video under it, work
    // along by an editor, climb its side, or peek and point at it.
    startTrip(now, activity) {
      this.lastTripAt = now;
      const target = this.active;
      const [min, max] = this.screenRange();
      const clamp = (x) => Math.min(max, Math.max(min, x));
      const centre = clamp(target.x + target.w / 2 - this.half);
      if (activity === "video") return this.goTo({ stage: "watch", active: true, targetX: centre });
      if (activity === "code" && Math.random() < 0.5) {
        const corner = Math.random() < 0.5 ? target.x + 60 : target.x + target.w - 60;
        return this.goTo({ stage: "work", active: true, targetX: clamp(corner - this.half) });
      }
      if (Math.random() < CLIMB_SHARE) {
        const climb = this.windowClimb(target);
        if (climb) return this.goTo(climb);
      }
      if (Math.random() < 0.5) {
        const edge = Math.random() < 0.5 ? target.x + 30 : target.x + target.w - 30;
        return this.goTo({ stage: "peek", active: true, targetX: clamp(edge - this.half) });
      }
      return this.goTo({ stage: Math.random() < 0.5 ? "point" : "peek", active: true, targetX: centre });
    }

    // Climb the nearer side of the active window, maximised ones too: with room on top
    // she may go over and stand on it, otherwise she hangs on its side at a random height.
    windowClimb(target) {
      const [min, max] = this.screenRange();
      const slack = this.canvasLeft + CLIMB_REACH.leftSide; // her window may hang off screen at an edge
      const sides = [
        { side: "left", x: target.x - this.canvasLeft - CLIMB_REACH.leftSide },
        { side: "right", x: target.x + target.w - this.canvasLeft - CLIMB_REACH.rightSide },
      ]
        .filter((option) => option.x >= min - slack && option.x <= max + slack)
        .sort((a, b) => Math.abs(a.x - this.x) - Math.abs(b.x - this.x));
      const top = Math.max(this.highestCling(), target.y - this.feet);
      const low = this.floorY() - 60;
      if (!sides.length || low <= top) return null;
      const roomOnTop = target.y - this.feet >= this.screen.top;
      const partway = !roomOnTop || Math.random() < WINDOW_CLING_CHANCE ? between([top, low]) : null;
      return {
        stage: "walk", active: true, side: sides[0].side, frame: target, targetX: sides[0].x, stopAt: partway,
      };
    }

    // Walk to a screen edge and climb it to a random height, 15% to 95% of the way up.
    startWallClimb() {
      const wall = Math.random() < 0.5 ? "left" : "right";
      const targetX = wall === "left"
        ? this.screen.left - this.canvasLeft - CLIMB_REACH.rightSide
        : this.screen.right - this.canvasLeft - CLIMB_REACH.leftSide;
      const top = this.highestCling();
      const from = this.y;
      if (from - top < 80) return false;
      const stopAt = from - (0.15 + Math.random() * 0.8) * (from - top);
      // On the screen's left edge the wall is on her left, like a window's right side.
      return this.goTo({ stage: "walk", side: wall === "left" ? "right" : "left", frame: null, targetX, stopAt });
    }

    // A harmless prank: start the window effect exactly on its sprite impact frame.
    // Only the decorative effect window draws it; the app underneath is untouched.
    startMischief(now) {
      if (!this.invoke || !this.canMischief() || now - this.lastMischiefAt < MISCHIEF_COOLDOWN) return false;
      // The effect has to come out of her cast: aim it at the window under her hand, or at
      // the space around her when there's none, so it starts where her hand is.
      const hand = this.handPoint();
      const under = this.windows.find((win) => hand.x >= win.x && hand.x <= win.x + win.w
        && hand.y >= win.y + 20 && hand.y <= win.y + win.h - 20);
      const frame = under || this.areaAround(hand);
      // Next to the active window (in front of it or just under its bottom edge): grab it.
      const active = this.active;
      const nearActive = Boolean(active) && hand.x >= active.x && hand.x <= active.x + active.w
        && hand.y >= active.y && hand.y <= active.y + active.h + 120;
      const pool = nearActive && Math.random() < GRAB_SHARE ? GRABS : MISCHIEF;
      let prank = pool[Math.floor(Math.random() * pool.length)];
      if (under && pool === MISCHIEF && Math.random() < BREAK_SHARE) return this.startBreakUi(now, under);
      this.lastMischiefAt = now;
      this.phase = "rest";
      this.looking = "prank";
      this.beatEnd = now + this.prankLength(prank);
      const area = FROM_HAND.has(prank.effect) ? this.areaAround(hand, prank.effect) : frame;
      this.prank = { ...prank, frame: area, event: prank.event || (prank.effect === "bend" ? "bend_peak" : "effect_start") };
      this.show(this.sprites.manifest?.clips[prank.clip] ? prank.clip : LOOKS.prank.clip, { loop: false });
      return true;
    }

    // Snapshot the window first (it takes a moment), then cast; the collapse or hijack fires
    // on her effect frame like any other prank. Without Screen Recording the collapse turns
    // into another prank, and the hijack glitches without the window's own pixels.
    startBreakUi(now, win) {
      const prank = Math.random() < 0.5 ? HIJACK : BREAK_UI;
      const frame = { x: win.x, y: win.y, w: win.w, h: win.h };
      this.lastMischiefAt = now;
      this.phase = "rest";
      this.looking = "prank-wait";
      this.beatEnd = now + 4000;
      this.show(this.emotionClip("surprised") || "core/idle");
      this.invoke("prepare_break_ui", { windowId: win.id })
        .then(() => {
          if (this.looking !== "prank-wait") return;
          this.cast(prank, frame);
        })
        .catch(() => {
          if (this.looking !== "prank-wait") return;
          this.cast(prank === HIJACK ? HIJACK : MISCHIEF[Math.floor(Math.random() * MISCHIEF.length)], frame);
        });
      return true;
    }

    // From the start of her cast until both her pose and the effect it fires are over.
    prankLength(prank) {
      const frames = this.sprites.manifest?.clips[prank.clip]?.frames || [];
      const total = frames.reduce((sum, item) => sum + item.duration, 0);
      const event = prank.event || (prank.effect === "bend" ? "bend_peak" : "effect_start");
      const cue = Math.max(0, frames.findIndex((f) => f.event === event));
      const fires = frames.slice(0, cue).reduce((sum, item) => sum + item.duration, 0);
      return Math.max(total, fires + (EFFECT_MS[prank.grab || prank.effect] || 1200)) + 200;
    }

    cast(prank, frame) {
      const clip = this.sprites.manifest?.clips[prank.clip];
      this.phase = "rest";
      this.looking = "prank";
      this.beatEnd = performance.now() + this.prankLength(prank);
      const area = FROM_HAND.has(prank.effect) ? this.areaAround(this.handPoint(), prank.effect) : frame;
      this.prank = { ...prank, frame: area, event: prank.event || (prank.effect === "bend" ? "bend_peak" : "effect_start") };
      this.show(clip ? prank.clip : LOOKS.prank.clip, { loop: false });
    }

    // Where her casting hand is on screen right now (the casting frames reach out at about
    // 150, 105 of the 192x208 cell, shown at half size).
    handPoint() {
      return { x: this.x + this.canvasLeft + 75, y: this.y + this.canvasTop + 53 };
    }

    // A 640x420 patch of screen centred on her hand (bigger for the pixel scatter), kept on the display.
    areaAround(hand, effect) {
      const w = effect === "pixel_dissolve" ? 960 : 640;
      const h = effect === "pixel_dissolve" ? 640 : 420;
      const x = Math.min(this.screen.right - w, Math.max(this.screen.left, hand.x - w / 2));
      const y = Math.min(this.screen.bottom - h, Math.max(this.displayTop, hand.y - h / 2));
      return { x, y, w, h };
    }

    goTo(trip) {
      this.trip = trip;
      this.walkTo(trip.targetX);
      return true;
    }

    walkTo(targetX) {
      this.phase = "approach";
      this.dir = targetX < this.x ? -1 : 1;
      this.show(this.walkClip());
    }

    approach(dt, now) {
      const trip = this.trip;
      if (!trip) {
        this.nextBeat(now);
        return;
      }
      const stride = WALK_SPEED * dt;
      const dx = trip.targetX - this.x;
      if (Math.abs(dx) > stride) {
        const dir = Math.sign(dx);
        if (dir !== this.dir) {
          this.dir = dir;
          this.show(this.walkClip());
        }
        this.x += dir * stride;
        if (this.surface === "ledge" && this.support) this.y = this.support.y - this.feet;
        this.moveTo();
        return;
      }
      this.x = trip.targetX;
      this.moveTo();
      if (trip.stage === "walk") {
        // At a window side or screen edge: climb it.
        trip.stage = "climb";
        this.phase = "climb";
        this.show(trip.side === "left" ? "movement/climb_right" : "movement/climb_left");
      } else if (trip.stage === "step") {
        // Over the top edge: stand on the window.
        this.trip = null;
        const ledge = this.ledgeUnder(this.x);
        if (ledge) {
          this.surface = "ledge";
          this.support = ledge;
          this.nextBeat(now);
        } else {
          this.loseFooting();
        }
      } else {
        // At the window: turn round and watch, work along, peek or point for a while.
        const look = LOOKS[trip.stage] || LOOKS.peek;
        this.looking = trip.stage;
        this.trip = null;
        this.phase = "rest";
        this.beatEnd = now + between(look.ms);
        this.show(this.sprites.manifest?.clips[look.clip] ? look.clip : "core/idle");
      }
    }

    // A ballistic jump to a landing spot: crouch, fly with gravity, land.
    startJump(landing) {
      const x0 = this.x;
      const y0 = this.y;
      const peak = Math.min(y0, landing.y);
      const apex = Math.min(peak, Math.max(this.highestCling() - 10, peak - between(HOP_ARC)));
      const rise = Math.sqrt(2 * GRAVITY * Math.max(0, y0 - apex));
      const total = rise / GRAVITY + Math.sqrt((2 * Math.max(0, landing.y - apex)) / GRAVITY);
      this.jump = {
        x0, y0, landing, t: 0, crouch: CROUCH_S, total: Math.max(0.2, total), vy: -rise,
        vx: (landing.x - x0) / Math.max(0.2, total),
      };
      this.surface = "floor";
      this.support = null;
      this.phase = "jump";
      this.show("movement/land", { loop: false });
      this.sprites.seek(3); // her deepest crouch
      this.sprites.paused = true;
    }

    jumpStep(dt) {
      const jump = this.jump;
      if (!jump) {
        this.loseFooting();
        return;
      }
      if (jump.crouch > 0) {
        jump.crouch -= dt;
        if (jump.crouch <= 0) this.show("movement/fall");
        return;
      }
      jump.t = Math.min(jump.total, jump.t + dt);
      this.x = jump.x0 + jump.vx * jump.t;
      this.y = jump.y0 + jump.vy * jump.t + 0.5 * GRAVITY * jump.t * jump.t;
      this.moveTo();
      if (jump.t < jump.total) return;
      this.jump = null;
      this.x = jump.landing.x;
      this.y = jump.landing.y;
      this.moveTo();
      // Landing on a window top that is right there beats landing on thin air.
      const ledge = this.ledgeNear(this.x, this.y);
      if (ledge) {
        this.y = ledge.y - this.feet;
        this.surface = "ledge";
        this.support = ledge;
        this.lineY = null;
      } else if (this.y >= this.floorY() - 2) {
        this.y = this.floorY();
        this.surface = "floor";
        this.lineY = null;
      } else {
        this.surface = "line";
        this.lineY = this.y;
      }
      this.moveTo();
      this.land(true);
    }

    // Hop about 5% of the screen up or down and stand at that height, like climbing the
    // scene one step at a time. Never above the ceiling or below the floor.
    startHop() {
      const step = HOP_STEP * (this.screen.bottom - this.displayTop);
      const ceiling = this.highestCling();
      const floor = this.floorY();
      const canUp = this.y - step >= ceiling;
      const canDown = this.y < floor - 2;
      if (!canUp && !canDown) return false;
      const up = canUp && (!canDown || Math.random() < HOP_UP_CHANCE);
      const y = up ? this.y - step : Math.min(floor, this.y + step);
      const [min, max] = this.screenRange();
      const side = Math.random() < 0.5 ? -1 : 1;
      const x = Math.min(max, Math.max(min, this.x + side * between(HOP_REACH)));
      this.startJump({ x, y });
      return true;
    }

    // Climb a window side or a screen edge. trip.stopAt is the height to cling at on the
    // way; a window climb with no stop goes over the top and onto the window.
    climb(dt, now) {
      const trip = this.trip;
      if (!trip || (trip.frame && !sameWindow(trip.frame, this.active))) {
        this.loseFooting();
        return;
      }
      const roomOnTop = Boolean(trip.frame) && trip.frame.y - this.feet >= this.highestCling();
      const top = this.climbTop(trip);
      const goal = trip.stopAt == null ? top : Math.max(top, trip.stopAt);
      this.y = Math.max(goal, this.y - CLIMB_SPEED * dt);
      this.moveTo();
      if (this.y > goal) return;
      if (roomOnTop && trip.stopAt == null) {
        trip.stage = "step";
        const centre = trip.side === "left" ? trip.frame.x + LEDGE_MARGIN : trip.frame.x + trip.frame.w - LEDGE_MARGIN;
        trip.targetX = centre - this.half;
        this.phase = "approach";
        this.dir = trip.side === "left" ? 1 : -1;
        this.show(this.walkClip());
        return;
      }
      // Hang on where she is for a moment, frozen mid-pull.
      this.phase = "cling";
      this.beatEnd = now + between(CLING_MS);
      this.sprites.paused = true;
    }

    // After clinging: climb on to a new random height (or the window top), or let go and fall.
    afterCling() {
      this.sprites.paused = false;
      const trip = this.trip;
      const top = trip ? this.climbTop(trip) : 0;
      if (trip && this.y - top > 40 && Math.random() < CLIMB_ON_CHANCE) {
        const roomOnTop = Boolean(trip.frame) && trip.frame.y - this.feet >= this.highestCling();
        const overTheTop = roomOnTop && Math.random() < 0.6;
        trip.stopAt = overTheTop ? null : between([top, this.y - 40]);
        this.phase = "climb";
        this.show(trip.side === "left" ? "movement/climb_right" : "movement/climb_left");
        return;
      }
      this.loseFooting();
    }

    // Highest point of a climb: a window's top edge, or the highest she can reach.
    climbTop(trip) {
      return trip.frame ? Math.max(this.highestCling(), trip.frame.y - this.feet) : this.highestCling();
    }

    // Highest window y she can reach while her head stays on the display.
    highestCling() {
      return this.displayTop - this.canvasTop + 8;
    }

    walk(dt) {
      const [min, max] = this.range();
      const [screenMin, screenMax] = this.screenRange();
      let x = this.x + this.dir * WALK_SPEED * dt;
      const pastEnd = (this.dir < 0 && x <= min) || (this.dir > 0 && x >= max);
      const hopOff = this.surface === "ledge" && this.walkOff && x > screenMin && x < screenMax;
      if (pastEnd && !hopOff) {
        // Turn around at the end, like a shimeji reaching a wall.
        x = Math.min(max, Math.max(min, x));
        this.dir = -this.dir;
        this.walkOff = false;
        this.show(this.walkClip());
      }
      // Clamp only ahead of her, so after a wall climb she walks back in instead of jumping.
      this.x = this.dir < 0 ? Math.max(screenMin, x) : Math.min(screenMax, x);
      if (this.surface === "ledge") {
        const ledge = this.support && this.ledgeUnder(this.x, this.support.win.id);
        if (!ledge) {
          this.loseFooting();
          return;
        }
        this.support = ledge;
        this.y = ledge.y - this.feet;
      } else if (this.surface === "line") {
        this.y = this.lineY;
      } else {
        this.y = this.floorY();
      }
      this.moveTo();
    }

    loseFooting() {
      const roaming = this.phase !== null && this.phase !== "fall" && this.phase !== "land";
      this.trip = null;
      this.jump = null;
      this.surface = "floor";
      this.support = null;
      this.lineY = null;
      if (this.y < this.floorY() - 2) {
        this.startFall(roaming);
      } else if (roaming) {
        this.y = this.floorY();
        this.moveTo();
      }
    }

    startFall(animate) {
      this.trip = null;
      this.phase = "fall";
      this.vy = 0;
      this.animateFall = animate;
      if (animate) this.show("movement/fall");
      else this.clip = null;
    }

    // Fall with gravity onto the first window top or the floor under her.
    fall(dt) {
      if (this.animateFall && this.sprites.clip !== this.clip) this.animateFall = false;
      this.vy = Math.min(MAX_FALL_SPEED, this.vy + GRAVITY * dt);
      const from = this.y;
      const to = from + this.vy * dt;
      const centre = this.x + this.half;
      const catcher = this.ledges()
        .filter((ledge) => centre >= ledge.left + 8 && centre <= ledge.right - 8)
        .map((ledge) => ({ ledge, top: ledge.y - this.feet }))
        .filter(({ top }) => top >= from - 1 && top <= to)
        .sort((a, b) => a.top - b.top)[0];
      if (catcher) {
        this.y = catcher.top;
        this.surface = "ledge";
        this.support = catcher.ledge;
      } else if (to >= this.floorY()) {
        this.y = this.floorY();
        this.surface = "floor";
        this.support = null;
      } else {
        this.y = to;
        this.moveTo();
        return;
      }
      this.moveTo();
      this.land(this.animateFall);
    }

    land(animate) {
      if (animate && this.sprites.manifest?.clips["movement/land"]) {
        this.phase = "land";
        this.show("movement/land", { loop: false });
      } else {
        this.phase = "rest";
        this.release(true);
      }
    }

    release(restore) {
      const owned = this.phase !== null;
      this.sprites.paused = false;
      this.looking = null;
      this.prank = null;
      this.phase = null;
      this.clip = null;
      this.trip = null;
      this.jump = null;
      if (restore && owned) this.onRelease?.();
    }

    show(clip, options) {
      this.sprites.paused = false;
      this.clip = this.sprites.play(clip, options) ? clip : this.sprites.clip;
    }

    walkClip() {
      return this.dir < 0 ? "movement/walk_left" : "movement/walk_right";
    }

    emotionClip(name) {
      if (!name || name === "neutral") return null;
      return this.sprites.manifest?.emotions?.[name] || null;
    }

    // One setPosition in flight at a time; later frames overwrite the pending spot.
    moveTo() {
      this.pending = { x: this.x, y: this.y };
      if (!this.sending) this.flushMove();
    }

    flushMove() {
      const next = this.pending;
      this.pending = null;
      if (!next) return;
      this.sending = true;
      this.win.setPosition(new this.Position(next.x, next.y))
        .catch((error) => console.warn("Hermes-chan could not move", error))
        .finally(() => {
          this.sending = false;
          this.flushMove();
        });
    }
  }

  window.HermesWander = HermesWander;
})();
