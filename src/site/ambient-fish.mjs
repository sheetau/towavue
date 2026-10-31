// Original canvas implementation inspired by Dao Ho's typographic pond.
// See notices/ambient-fish.md. No pointer input, feeding, eyes, or barbels.
const TAU = Math.PI * 2;
const random = (min, max) => min + Math.random() * (max - min);
const clamp = (n, min, max) => Math.max(min, Math.min(max, n));
const smooth = (n) => { const t = clamp(n, 0, 1); return t * t * (3 - 2 * t); };
const BODY_PROFILE = [[0, 0], [0.055, 0.052], [0.19, 0.104], [0.34, 0.096], [0.53, 0.058], [0.72, 0.017], [0.77, 0.012]];
const BODY_GLYPHS = [".", ":", "+", "=", "x", "%", "#", "*"];
const FIN_GLYPHS = [".", "-", "/", "|", "\\", "-", "/", ":"];
const SHADES = 32;
const SPINE_POINTS = 30;
const SPINE_LENGTH = 1.12;
const RIPPLE_LIFE = 4.8;
const TAIL_PHASE = 0.96 * 7.4;

function halfWidth(s) {
  for (let i = 1; i < BODY_PROFILE.length; i++) {
    const [end, b] = BODY_PROFILE[i];
    if (s <= end) {
      const [start, a] = BODY_PROFILE[i - 1];
      return a + (b - a) * smooth((s - start) / (end - start));
    }
  }
  return BODY_PROFILE.at(-1)[1];
}

// Sample each fish once, preserving its identity and route through resizes.
// Only ranges are authored: no fixed spawn points, routes or pattern presets.
function makeFish(id, width, time) {
  const burstDuration = random(1.4, 2.5);
  const interval = burstDuration + random(2.2, 4.8);
  const burstStart = time - random(0, interval);
  const patternScale = random(0.7, 1.6);
  return {
    id, seed: random(0, TAU), angle: random(0, TAU),
    direction: Math.random() < 0.5 ? -1 : 1,
    centerX: random(0.44, 0.56), centerY: random(0.45, 0.56),
    orbitX: random(0.29, 0.37), orbitY: random(0.27, 0.36),
    routeRotation: random(0, TAU),
    harmonicX: Math.random() < 0.5 ? 2 : 3,
    harmonicY: Math.random() < 0.5 ? 2 : 3,
    distortionX: random(0.07, 0.15), distortionY: random(0.07, 0.15),
    phaseX: random(0, TAU), phaseY: random(0, TAU),
    size: random(0.85, 1.12), pace: random(0.82, 1.18),
    strokeRate: random(3.1, 4.2), tailStrength: random(0.85, 1.15),
    speed: (width < 700 ? 15 : 24) * random(0.8, 1.15),
    turn: 0, beat: random(0, TAU), activity: random(0.1, 0.5), bend: 0,
    burstStart, burstDuration, nextBurst: burstStart + interval,
    burstStrength: random(0.8, 1), nextRipple: time + random(0.2, 1.2),
    depthRate: random(0.045, 0.095), glyphOffset: Math.floor(random(0, 8)),
    patternBase: random(0.48, 0.78), patternWarp: random(0.004, 0.018),
    patches: Array.from({ length: Math.floor(random(4, 10)) }, () => ({
      along: random(0.02, 0.76), across: random(-0.085, 0.085),
      length: random(0.035, 0.13) * patternScale,
      width: random(0.025, 0.075) * patternScale,
      skew: random(-1.4, 1.4),
      tone: Math.random() < 0.75 ? random(-0.8, -0.35) : random(0.2, 0.45),
    })),
    spine: new Float32Array(SPINE_POINTS * 4),
  };
}

function pathAt(fish, angle, width, height) {
  // Random smooth closed curves avoid wrap/respawn seams and abrupt U-turns.
  const ax = fish.harmonicX * angle + fish.phaseX;
  const ay = fish.harmonicY * angle + fish.phaseY;
  const x = Math.cos(angle) + fish.distortionX * Math.sin(ax);
  const y = Math.sin(angle) + fish.distortionY * Math.cos(ay);
  const dx = -Math.sin(angle) + fish.harmonicX * fish.distortionX * Math.cos(ax);
  const dy = Math.cos(angle) - fish.harmonicY * fish.distortionY * Math.sin(ay);
  const c = Math.cos(fish.routeRotation);
  const s = Math.sin(fish.routeRotation);
  const rx = width * fish.orbitX;
  const ry = height * fish.orbitY;
  return {
    x: width * fish.centerX + rx * (x * c - y * s),
    y: height * fish.centerY + ry * (x * s + y * c),
    dx: rx * (dx * c - dy * s), dy: ry * (dx * s + dy * c),
  };
}

function makeAtlas(cw, ch, dpr) {
  const canvas = document.createElement("canvas");
  const tileW = Math.ceil(cw * dpr);
  const tileH = Math.ceil(ch * dpr);
  canvas.width = tileW * 24;
  canvas.height = tileH * SHADES;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.font = `${Math.round(ch * dpr * 0.86)}px ui-monospace, Menlo, Consolas, monospace`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (let level = 0; level < SHADES; level++) {
    const brightness = 4 + level * 2;
    for (let index = 0; index < 24; index++) {
      const x = index * tileW;
      const y = level * tileH;
      const wash = index < 8 ? brightness : Math.round(brightness * 0.35);
      if (index < 16) {
        ctx.fillStyle = `rgb(${wash},${wash},${wash})`;
        ctx.fillRect(x, y, tileW - Math.max(0.6, dpr * 0.55), tileH - Math.max(0.6, dpr * 0.55));
      }
      const ink = index < 8 ? Math.round(brightness * 0.42) : brightness;
      ctx.fillStyle = `rgb(${ink},${ink},${ink})`;
      ctx.fillText((index < 8 ? BODY_GLYPHS : FIN_GLYPHS)[index % 8], x + tileW / 2, y + tileH * 0.48);
    }
  }
  return { canvas, tileW, tileH };
}

// Trace the route behind the head at equal distances, then add a travelling
// wave. Turns bend the whole body along the actual route instead of rotating
// a rigid silhouette; each cross-section also rotates with its local spine.
function updateSpine(fish, width, height) {
  const spine = fish.spine;
  let angle = fish.angle;
  const step = fish.length * SPINE_LENGTH / (SPINE_POINTS - 1);
  for (let i = 0; i < SPINE_POINTS; i++) {
    const point = pathAt(fish, angle, width, height);
    const distance = Math.hypot(point.dx, point.dy);
    const heading = Math.atan2(point.dy * fish.direction, point.dx * fish.direction);
    const s = i / (SPINE_POINTS - 1) * SPINE_LENGTH;
    const effort = 0.08 + fish.activity * 0.92;
    const amplitude = (0.009 * s + 0.055 * s * s) * effort * fish.tailStrength * (1 + fish.bend * 0.65);
    const wave = Math.sin(fish.beat - s * 7.4) * amplitude * fish.length;
    spine[i * 4] = point.x - Math.sin(heading) * wave;
    spine[i * 4 + 1] = point.y + Math.cos(heading) * wave;
    angle -= fish.direction * step / Math.max(1, distance);
  }
  for (let i = 0; i < SPINE_POINTS; i++) {
    const before = Math.max(0, i - 1) * 4;
    const after = Math.min(SPINE_POINTS - 1, i + 1) * 4;
    const dx = spine[before] - spine[after];
    const dy = spine[before + 1] - spine[after + 1];
    const length = Math.hypot(dx, dy) || 1;
    spine[i * 4 + 2] = -dy / length;
    spine[i * 4 + 3] = dx / length;
  }
}

function bodyPoint(fish, s, lateral = 0) {
  const index = clamp(s / SPINE_LENGTH * (SPINE_POINTS - 1), 0, SPINE_POINTS - 1);
  const a = Math.min(SPINE_POINTS - 2, Math.floor(index)) * 4;
  const mix = index - a / 4;
  const spine = fish.spine;
  const x = spine[a] + (spine[a + 4] - spine[a]) * mix;
  const y = spine[a + 1] + (spine[a + 5] - spine[a + 1]) * mix;
  const nx = spine[a + 2] + (spine[a + 6] - spine[a + 2]) * mix;
  const ny = spine[a + 3] + (spine[a + 7] - spine[a + 3]) * mix;
  const scale = lateral * fish.length / (Math.hypot(nx, ny) || 1);
  return [x + nx * scale, y + ny * scale];
}

// Project a tile onto the bent spine so the soft markings stay on the fish
// while it turns, rather than sliding across a straight, screen-space axis.
function bodyCoordinates(fish, x, y, out) {
  let nearest = Infinity;
  const spine = fish.spine;
  for (let i = 0; i < SPINE_POINTS - 1; i++) {
    const offset = i * 4;
    const dx = spine[offset + 4] - spine[offset];
    const dy = spine[offset + 5] - spine[offset + 1];
    const t = clamp(((x - spine[offset]) * dx + (y - spine[offset + 1]) * dy) / (dx * dx + dy * dy || 1), 0, 1);
    const px = x - spine[offset] - dx * t;
    const py = y - spine[offset + 1] - dy * t;
    const squared = px * px + py * py;
    if (squared < nearest) {
      nearest = squared;
      out.along = (i + t) / (SPINE_POINTS - 1) * SPINE_LENGTH;
      out.across = (px * spine[offset + 2] + py * spine[offset + 3]) / fish.length;
    }
  }
}

function bodyMarkings(fish, along, across) {
  // Uneven, asymmetric patches remain attached to the bent body. Their size,
  // density, skew and pale/dark balance differ without increasing overall contrast.
  const warped = across + Math.sin(along * 19 + fish.seed) * fish.patternWarp;
  let tone = fish.patternBase;
  for (const patch of fish.patches) {
    const y = warped - patch.across;
    const x = along - patch.along + y * patch.skew;
    const distance = (x / patch.length) ** 2 + (y / patch.width) ** 2;
    tone += patch.tone * (1 - smooth(distance));
  }
  return clamp(tone, 0, 1);
}

function drawSilhouette(ctx, fish) {
  const point = (s, lateral = 0) => bodyPoint(fish, s, lateral);
  const move = (s, v) => ctx.moveTo(...point(s, v));
  const curve = (a, b, c, d, e, f) => ctx.bezierCurveTo(...point(a, b), ...point(c, d), ...point(e, f));
  // The blue channel identifies the fish in the tiny offscreen mask.
  const id = (fish.id + 1) * 32;
  ctx.fillStyle = `rgb(0,255,${id})`;
  ctx.beginPath();
  move(0.71, 0.014);
  curve(0.83, 0.025, 0.95, 0.16, 1.08, 0.17);
  curve(1.03, 0.075, 0.99, 0.025, 0.96, 0);
  curve(0.99, -0.025, 1.03, -0.075, 1.08, -0.17);
  curve(0.95, -0.16, 0.83, -0.025, 0.71, -0.014);
  ctx.closePath();
  ctx.fill();
  for (const side of [-1, 1]) {
    const spread = 1 + 0.07 * Math.sin(fish.beat + side * 0.4) * fish.activity + fish.bend * 0.12;
    ctx.beginPath();
    move(0.20, side * 0.07);
    curve(0.21, side * 0.13 * spread, 0.33, side * 0.205 * spread, 0.44, side * 0.17 * spread);
    curve(0.43, side * 0.105, 0.36, side * 0.06, 0.28, side * 0.055);
    ctx.closePath();
    ctx.fill();
    ctx.beginPath();
    move(0.49, side * 0.045);
    curve(0.53, side * 0.105, 0.60, side * 0.11, 0.65, side * 0.075);
    curve(0.59, side * 0.06, 0.55, side * 0.04, 0.49, side * 0.045);
    ctx.closePath();
    ctx.fill();
  }
  ctx.fillStyle = `rgb(255,0,${id})`;
  ctx.beginPath();
  move(0, 0);
  for (let i = 1; i <= 28; i++) {
    const s = i / 28 * 0.77;
    ctx.lineTo(...point(s, halfWidth(s)));
  }
  for (let i = 28; i >= 0; i--) {
    const s = i / 28 * 0.77;
    ctx.lineTo(...point(s, -halfWidth(s)));
  }
  ctx.closePath();
  ctx.fill();
}

export function createAmbientFish(canvas) {
  const ctx = canvas.getContext("2d", { alpha: true });
  const mask = document.createElement("canvas");
  const maskCtx = mask.getContext("2d", { willReadFrequently: true });
  if (!ctx || !maskCtx) return null;
  let width = 0;
  let height = 0;
  let cw = 10;
  let ch = 10;
  let dpr = 1;
  let atlas = null;
  let time = 0;
  let fish = [];
  const ripples = [];
  const coordinates = { along: 0, across: 0 };

  function resize() {
    const rect = canvas.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) return;
    for (const ripple of ripples) {
      ripple.x *= rect.width / (width || rect.width);
      ripple.y *= rect.height / (height || rect.height);
    }
    width = rect.width;
    height = rect.height;
    dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    cw = ch = width < 600 ? 8 : 10;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    mask.width = Math.ceil(width / cw);
    mask.height = Math.ceil(height / ch);
    atlas = makeAtlas(cw, ch, dpr);
    const count = width < 700 ? 2 : width < 1500 ? 3 : 4;
    fish = Array.from({ length: count }, (_, id) => fish[id] ?? makeFish(id, width, time));
    for (const f of fish) {
      f.length = clamp(Math.min(width, height) * 0.32, 125, 255) * f.size;
      const p = pathAt(f, f.angle, width, height);
      f.x = p.x;
      f.y = p.y;
      f.heading = Math.atan2(p.dy * f.direction, p.dx * f.direction);
    }
  }

  function draw(dt = 0) {
    if (!atlas || !width || !height) return;
    time += dt;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    maskCtx.setTransform(1, 0, 0, 1, 0, 0);
    maskCtx.clearRect(0, 0, mask.width, mask.height);
    maskCtx.setTransform(1 / cw, 0, 0, 1 / ch, 0, 0);
    for (const f of fish) {
      const previous = pathAt(f, f.angle, width, height);
      // Anticipate a bend, so steering effort reaches the tail before the turn.
      const ahead = pathAt(f, f.angle + f.direction * f.length * 0.45 / Math.max(1, Math.hypot(previous.dx, previous.dy)), width, height);
      const bendAngle = Math.atan2(previous.dx * ahead.dy - previous.dy * ahead.dx,
        previous.dx * ahead.dx + previous.dy * ahead.dy);
      f.bend += (clamp(Math.abs(bendAngle) / 0.65, 0, 1) - f.bend) * (1 - Math.exp(-dt * 2.5));
      // Short bouts of strokes alternate with a relaxed glide. Curves call for
      // extra strokes; the same effort drives the spine, propulsion and wake.
      if (dt > 0 && time >= f.nextBurst) {
        f.burstStart = time;
        f.burstDuration = random(1.4, 2.5);
        f.burstStrength = random(0.8, 1);
        f.nextBurst = time + f.burstDuration + random(2.2, 4.8);
      }
      const cycle = time - f.burstStart;
      const burst = smooth(cycle / 0.5) * (1 - smooth((cycle - f.burstDuration + 0.7) / 0.7)) * f.burstStrength;
      const effort = Math.max(burst, smooth((f.bend - 0.18) / 0.7) * 0.85);
      f.activity += (effort - f.activity) * (1 - Math.exp(-dt * 3.5));
      const previousBeat = f.beat;
      f.beat += dt * (0.45 + f.activity * (f.strokeRate + f.speed * 0.035));
      const sweep = Math.abs(Math.cos(f.beat - TAIL_PHASE));
      const targetSpeed = (width < 700 ? 15 : 24) * f.pace * (0.65 + f.activity * (0.9 + 0.16 * sweep ** 4));
      const response = targetSpeed > f.speed ? 2.4 : 0.55;
      f.speed += (targetSpeed - f.speed) * (1 - Math.exp(-dt * response));
      f.angle += f.direction * f.speed / Math.max(1, Math.hypot(previous.dx, previous.dy)) * dt;
      const p = pathAt(f, f.angle, width, height);
      const targetHeading = Math.atan2(p.dy * f.direction, p.dx * f.direction);
      const delta = Math.atan2(Math.sin(targetHeading - f.heading), Math.cos(targetHeading - f.heading));
      const rotation = delta * (1 - Math.exp(-dt * 2.8));
      f.heading += rotation;
      f.turn += ((dt ? rotation / dt : 0) - f.turn) * (1 - Math.exp(-dt * 2));
      f.x = p.x;
      f.y = p.y;
      updateSpine(f, width, height);
      // Emit only as a working tail crosses its mid-stroke, where it pushes
      // hardest, instead of releasing unrelated timer-driven rings.
      const stroke = Math.floor((f.beat - TAIL_PHASE) / Math.PI) !== Math.floor((previousBeat - TAIL_PHASE) / Math.PI);
      if (dt > 0 && stroke && f.activity > 0.48 && time >= f.nextRipple) {
        const [x, y] = bodyPoint(f, 0.96);
        ripples.push({ x, y, born: time, strength: 0.7 + f.activity * 0.15 + f.bend * 0.15 });
        if (ripples.length > 12) ripples.shift();
        f.nextRipple = time + 0.95 - f.bend * 0.2;
      }
      drawSilhouette(maskCtx, f);
    }
    drawRipples();
    const pixels = maskCtx.getImageData(0, 0, mask.width, mask.height).data;
    for (let row = 0; row < mask.height; row++) {
      for (let col = 0; col < mask.width; col++) {
        const index = (row * mask.width + col) * 4;
        if (pixels[index + 3] < 12) continue;
        const f = fish[clamp(Math.round(pixels[index + 2] / 32) - 1, 0, fish.length - 1)];
        const x = (col + 0.5) * cw;
        const y = (row + 0.5) * ch;
        bodyCoordinates(f, x, y, coordinates);
        const { along, across } = coordinates;
        const body = pixels[index] >= pixels[index + 1];
        const markings = bodyMarkings(f, along, across);
        const ridge = 1 - clamp(Math.abs(across) / 0.12, 0, 1);
        // Retain the soft pattern, with a modest increase in dark-area contrast.
        const brightness = body ? 23 + ridge * 21 + markings * 14 - 7 * (1 - markings) ** 2 : 36 + markings * 12;
        const level = clamp(Math.round((brightness - 4) / 2), 0, SHADES - 1);
        const pattern = Math.abs(Math.floor(along * 39) * 7 + Math.floor(across * 80) * 3 + f.glyphOffset) % 8;
        const glyph = pattern + (body ? 0 : 8);
        // The quiet center protects text without any scroll-linked movement.
        const edge = smooth(Math.abs(x / width - 0.5) / 0.43);
        const depth = 0.78 + 0.16 * Math.sin(time * f.depthRate + f.seed);
        ctx.globalAlpha = pixels[index + 3] / 255 * (0.52 + edge * 0.48) * depth;
        ctx.drawImage(atlas.canvas, glyph * atlas.tileW, level * atlas.tileH, atlas.tileW, atlas.tileH, col * cw, row * ch, cw, ch);
      }
    }
    ctx.globalAlpha = 1;
  }

  function drawRipples() {
    for (let i = ripples.length - 1; i >= 0; i--) {
      const ripple = ripples[i];
      const age = time - ripple.born;
      if (age >= RIPPLE_LIFE) { ripples.splice(i, 1); continue; }
      // Two sparse glyph rings spread from each tail wake and fade into black.
      for (const delay of [0, 0.48]) {
        const t = age - delay;
        if (t < 0) continue;
        const radius = 12 + t * 24;
        const count = Math.max(12, Math.round(TAU * radius / (cw * 1.45)));
        const fade = smooth(t / 0.35) * (1 - t / RIPPLE_LIFE) ** 1.5;
        for (let j = 0; j < count; j++) {
          const angle = j / count * TAU;
          const dx = -Math.sin(angle);
          const dy = Math.cos(angle);
          const glyph = Math.abs(dx) < 0.38 ? 3 : Math.abs(dy) < 0.38 ? 1 : dx * dy > 0 ? 4 : 2;
          const x = Math.round((ripple.x + Math.cos(angle) * radius) / cw) * cw;
          const y = Math.round((ripple.y + Math.sin(angle) * radius) / ch) * ch;
          const edge = smooth(Math.abs(x / width - 0.5) / 0.43);
          ctx.globalAlpha = fade * (0.55 + edge * 0.20) * ripple.strength;
          ctx.drawImage(atlas.canvas, (16 + glyph) * atlas.tileW, 28 * atlas.tileH,
            atlas.tileW, atlas.tileH, x, y, cw, ch);
        }
      }
    }
    ctx.globalAlpha = 1;
  }

  return {
    resize, draw,
    destroy() {
      fish = [];
      ripples.length = 0;
      atlas = null;
      mask.width = mask.height = 1;
      canvas.width = canvas.height = 1;
    },
  };
}
