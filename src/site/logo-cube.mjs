// Eighteen solid rectangular rods follow the SVG strokes on six cube faces.
// The outside of each rod stays on its face; thickness extends inward.
export const LOGO_SPIN_DURATION = 1300;
export const LOGO_FADE_DURATION = 320;
export const LOGO_IDLE_DELAY = 3000;

export function cubeVertices(strokes, viewBox, thickness) {
  const faces = [
    [
      [0, 0, 1],
      [1, 0, 0],
      [0, 1, 0],
      [1, 1],
    ],
    [
      [0, 0, -1],
      [-1, 0, 0],
      [0, 1, 0],
      [-1, 1],
    ],
    [
      [1, 0, 0],
      [0, 0, -1],
      [0, 1, 0],
      [1, 1],
    ],
    [
      [-1, 0, 0],
      [0, 0, 1],
      [0, 1, 0],
      [-1, 1],
    ],
    [
      [0, 1, 0],
      [1, 0, 0],
      [0, 0, -1],
      [1, -1],
    ],
    [
      [0, -1, 0],
      [1, 0, 0],
      [0, 0, 1],
      [1, 1],
    ],
  ];
  const vertices = [];
  // Mirror opposite artwork as before. Right/bottom faces match the SVG
  // after left/up quarter-turns; the extrusion never changes the front outline.
  for (const [normal, right, up, orientation] of faces) {
    const position = ([x, y], depth) => {
      const u = (((x - viewBox.x) / viewBox.width) * 2 - 1) * orientation[0];
      const v = (1 - ((y - viewBox.y) / viewBox.height) * 2) * orientation[1];
      return normal.map((n, i) => n * (1 - depth) + right[i] * u + up[i] * v);
    };
    for (const { x1, y1, x2, y2, width } of strokes) {
      const dx = x2 - x1;
      const dy = y2 - y1;
      const length = Math.hypot(dx, dy);
      if (!length || width <= 0) continue;
      const ox = ((-dy / length) * width) / 2;
      const oy = ((dx / length) * width) / 2;
      const outline = [
        [x1 + ox, y1 + oy],
        [x2 + ox, y2 + oy],
        [x2 - ox, y2 - oy],
        [x1 - ox, y1 - oy],
      ];
      const corners = [
        ...outline.map((point) => position(point, 0)),
        ...outline.map((point) => position(point, thickness)),
      ];
      const center = [0, 1, 2].map((i) => corners.reduce((sum, p) => sum + p[i], 0) / 8);
      for (const indices of [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
      ]) {
        let points = indices.map((index) => corners[index]);
        const a = points[1].map((n, i) => n - points[0][i]);
        const b = points[2].map((n, i) => n - points[0][i]);
        let surfaceNormal = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        const size = Math.hypot(...surfaceNormal);
        surfaceNormal = surfaceNormal.map((n) => n / size);
        // Mirrored artwork can reverse winding; always face out of the rod.
        if (surfaceNormal.reduce((dot, n, i) => dot + n * (points[0][i] - center[i]), 0) < 0) {
          points = points.toReversed();
          surfaceNormal = surfaceNormal.map((n) => -n);
        }
        for (const index of [0, 1, 2, 0, 2, 3]) vertices.push(...points[index], ...surfaceNormal);
      }
    }
  }
  return new Float32Array(vertices);
}

export function spinAngle(progress) {
  const t = Math.max(0, Math.min(1, progress));
  // Time-warp a damped spring for a gentle launch, a fast sweep, then
  // roughly 8.5 degrees of overshoot and a small (~0.8 degree) rebound.
  const decay = 9;
  const frequency = 12;
  const response = (time) =>
    1 - Math.exp(-decay * time) * (Math.cos(frequency * time) + (decay / frequency) * Math.sin(frequency * time));
  // Normalize the endpoint so the SVG handoff lands at exactly 90 degrees.
  return (response(t ** 2.5) / response(1)) * (Math.PI / 2);
}

const VERTEX = `
attribute vec3 a_position;
attribute vec3 a_normal;
uniform float u_angle;
uniform vec3 u_axis;
varying float v_depth;
varying vec3 v_normal;
void main() {
  // Negative Y moves the front left; negative X moves it upward.
  float c = cos(u_angle);
  float s = sin(u_angle);
  vec3 p = a_position * c + cross(u_axis, a_position) * s
    + u_axis * dot(u_axis, a_position) * (1.0 - c);
  // Mild perspective, with the outside of the front rods at the SVG's size.
  float perspective = (12.0 - p.z) / 11.0;
  gl_Position = vec4(p.xy * 0.5, -p.z * 0.1, perspective);
  v_depth = p.z;
  v_normal = a_normal * c + cross(u_axis, a_normal) * s
    + u_axis * dot(u_axis, a_normal) * (1.0 - c);
}`;

const FRAGMENT = `
precision mediump float;
uniform vec3 u_color;
varying float v_depth;
varying vec3 v_normal;
void main() {
  // Preserve the current depth fade, including its half-bright rear limit.
  float fog = smoothstep(-1.0, 1.0, v_depth);
  // Subtle shading reveals the prism sides; front-facing strokes stay white.
  float shade = 0.78 + 0.22 * abs(normalize(v_normal).z);
  gl_FragColor = vec4(u_color * shade * fog, fog);
}`;

export function createLogoCubeRenderer(canvas, svg) {
  const gl = canvas.getContext("webgl", { alpha: true, antialias: true, depth: true, premultipliedAlpha: true });
  if (!gl || gl.isContextLost()) return null;
  const shaders = [];
  const program = gl.createProgram();
  const buffer = gl.createBuffer();
  const destroy = () => {
    shaders.forEach((shader) => gl.deleteShader(shader));
    gl.deleteProgram(program);
    gl.deleteBuffer(buffer);
  };
  try {
    if (!program || !buffer) throw new Error("Logo GPU allocation failed");
    for (const [type, source] of [
      [gl.VERTEX_SHADER, VERTEX],
      [gl.FRAGMENT_SHADER, FRAGMENT],
    ]) {
      const shader = gl.createShader(type);
      if (!shader) throw new Error("Logo shader allocation failed");
      shaders.push(shader);
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error("Logo shader compilation failed");
      gl.attachShader(program, shader);
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error("Logo shader link failed");
    gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    // Read the actual SVG lines once so the resting logo and rods cannot drift.
    const strokes = [...svg.querySelectorAll("line")].map((line) => ({
      x1: line.x1.baseVal.value,
      y1: line.y1.baseVal.value,
      x2: line.x2.baseVal.value,
      y2: line.y2.baseVal.value,
      width: parseFloat(getComputedStyle(line).strokeWidth),
    }));
    const logoWidth = Math.max(1, svg.getBoundingClientRect().width);
    const vertices = cubeVertices(strokes, svg.viewBox.baseVal, 2 / logoWidth);
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
    for (const [name, offset] of [
      ["a_position", 0],
      ["a_normal", 12],
    ]) {
      const attribute = gl.getAttribLocation(program, name);
      gl.enableVertexAttribArray(attribute);
      gl.vertexAttribPointer(attribute, 3, gl.FLOAT, false, 24, offset);
    }
    const rgb = getComputedStyle(svg).color.match(/[\d.]+/g);
    gl.uniform3fv(
      gl.getUniformLocation(program, "u_color"),
      rgb?.length >= 3 ? rgb.slice(0, 3).map((n) => Number(n) / 255) : [1, 1, 1],
    );
    gl.enable(gl.CULL_FACE);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.clearColor(0, 0, 0, 0);
    const angle = gl.getUniformLocation(program, "u_angle");
    const axis = gl.getUniformLocation(program, "u_axis");
    return {
      destroy,
      draw(rotation, direction) {
        const dpr = Math.min(window.devicePixelRatio || 1, 3);
        const size = Math.max(1, Math.round(canvas.clientWidth * dpr));
        if (canvas.width !== size || canvas.height !== size) {
          canvas.width = canvas.height = size;
          gl.viewport(0, 0, size, size);
        }
        gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
        gl.uniform1f(angle, rotation);
        gl.uniform3f(axis, direction === "up" ? -1 : 0, direction === "left" ? -1 : 0, 0);
        // Resolve the nearest solid surface first. Faded rear surfaces must not
        // add brightness through the front of a rod or at rod intersections.
        gl.colorMask(false, false, false, false);
        gl.drawArrays(gl.TRIANGLES, 0, vertices.length / 6);
        gl.colorMask(true, true, true, true);
        gl.drawArrays(gl.TRIANGLES, 0, vertices.length / 6);
      },
    };
  } catch {
    destroy();
    return null;
  }
}
