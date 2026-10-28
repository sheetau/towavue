// A real mesh with six transparent SVG faces. Geometry is separate from the
// renderer so later versions can replace the planes with tubes/blended corners.
export const LOGO_SPIN_DURATION = 1800;
export const LOGO_FADE_DURATION = 320;
export const LOGO_IDLE_DELAY = 8000;

export function cubeVertices() {
  const faces = [
    [[0, 0, 1], [1, 0, 0], [0, 1, 0], [1, 1]],
    [[0, 0, -1], [-1, 0, 0], [0, 1, 0], [-1, 1]],
    [[1, 0, 0], [0, 0, -1], [0, 1, 0], [1, 1]],
    [[-1, 0, 0], [0, 0, 1], [0, 1, 0], [-1, 1]],
    [[0, 1, 0], [1, 0, 0], [0, 0, -1], [1, 1]],
    [[0, -1, 0], [1, 0, 0], [0, 0, 1], [1, -1]],
  ];
  // Mirror the UVs on the opposite face so the artwork aligns when viewed
  // through the cube, including the corner without a diagonal stroke.
  return new Float32Array(faces.flatMap(([normal, right, up, uvDirection]) =>
    [[-1, -1], [1, -1], [1, 1], [-1, -1], [1, 1], [-1, 1]].flatMap(([x, y]) => [
      ...normal.map((n, i) => n + x * right[i] + y * up[i]),
      (x * uvDirection[0] + 1) / 2, (y * uvDirection[1] + 1) / 2,
    ])));
}

export function spinAngle(progress) {
  const t = Math.max(0, Math.min(1, progress));
  // A smooth quartic ease holds the starting pose longer, accelerates
  // strongly through the middle, then settles gently into the original pose.
  const entry = t ** 4;
  const exit = (1 - t) ** 4;
  return (entry / (entry + exit)) * Math.PI * 2;
}

const VERTEX = `
attribute vec3 a_position;
attribute vec2 a_uv;
uniform float u_angle;
varying vec2 v_uv;
void main() {
  // The front travels from lower left toward upper right (screen coordinates).
  vec3 axis = normalize(vec3(-1.0, 1.0, 0.0));
  float c = cos(u_angle);
  float s = sin(u_angle);
  vec3 p = a_position * c + cross(axis, a_position) * s
    + axis * dot(axis, a_position) * (1.0 - c);
  // Orthographic projection: depth never changes the apparent scale.
  gl_Position = vec4(p.xy * 0.5, -p.z * 0.1, 1.0);
  v_uv = a_uv;
}`;

const FRAGMENT = `
precision mediump float;
uniform sampler2D u_logo;
varying vec2 v_uv;
void main() {
  vec4 ink = texture2D(u_logo, v_uv);
  if (ink.a < 0.01) discard;
  gl_FragColor = vec4(ink.rgb * ink.a, ink.a);
}`;

export async function createLogoCubeRenderer(canvas, svg) {
  // Rasterize the existing SVG once, not on each animation frame.
  const source = svg.cloneNode(true);
  source.setAttribute("width", "256");
  source.setAttribute("height", "256");
  source.setAttribute("stroke", getComputedStyle(svg).color);
  const image = new Image();
  image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(source))}`;
  await image.decode();
  const gl = canvas.getContext("webgl", { alpha: true, antialias: true, premultipliedAlpha: true });
  if (!gl || gl.isContextLost()) return null;
  const shaders = [];
  const program = gl.createProgram();
  const buffer = gl.createBuffer();
  const texture = gl.createTexture();
  const destroy = () => {
    shaders.forEach((shader) => gl.deleteShader(shader));
    gl.deleteProgram(program);
    gl.deleteBuffer(buffer);
    gl.deleteTexture(texture);
  };
  try {
    if (!program || !buffer || !texture) throw new Error("Logo GPU allocation failed");
    for (const [type, source] of [[gl.VERTEX_SHADER, VERTEX], [gl.FRAGMENT_SHADER, FRAGMENT]]) {
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
    const vertices = cubeVertices();
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
    for (const [name, size, offset] of [["a_position", 3, 0], ["a_uv", 2, 12]]) {
      const attribute = gl.getAttribLocation(program, name);
      gl.enableVertexAttribArray(attribute);
      gl.vertexAttribPointer(attribute, size, gl.FLOAT, false, 20, offset);
    }
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.uniform1i(gl.getUniformLocation(program, "u_logo"), 0);
    // Both sides of every transparent face remain visible. Neither the planes
    // nor their transparent pixels hide the artwork on the far side.
    gl.disable(gl.CULL_FACE);
    gl.disable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.clearColor(0, 0, 0, 0);
    const angle = gl.getUniformLocation(program, "u_angle");
    return {
      destroy,
      draw(rotation) {
        const dpr = Math.min(window.devicePixelRatio || 1, 3);
        const size = Math.max(1, Math.round(canvas.clientWidth * dpr));
        if (canvas.width !== size || canvas.height !== size) {
          canvas.width = canvas.height = size;
          gl.viewport(0, 0, size, size);
        }
        gl.clear(gl.COLOR_BUFFER_BIT);
        gl.uniform1f(angle, rotation);
        gl.drawArrays(gl.TRIANGLES, 0, vertices.length / 5);
      },
    };
  } catch {
    destroy();
    return null;
  }
}
