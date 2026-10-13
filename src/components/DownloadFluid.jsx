import { useEffect, useRef } from "react";
import { FRAG, VERT } from "../site/fluid-shaders.mjs";

function tokenColor(styles, token, fallback) {
  let hex = styles.getPropertyValue(token).trim().replace(/^#/, "");
  if (hex.length === 3) hex = [...hex].map((part) => part + part).join("");
  if (!/^[\da-f]{6}$/i.test(hex)) return fallback;
  return [0, 2, 4].map((index) => parseInt(hex.slice(index, index + 2), 16) / 255);
}

function createRenderer(canvas, button) {
  const gl = canvas.getContext("webgl", { alpha: false, antialias: false, depth: false });
  if (!gl) return null;
  const shaders = [];
  const compile = (type, source) => {
    const shader = gl.createShader(type);
    if (!shader) return null;
    shaders.push(shader);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    return gl.getShaderParameter(shader, gl.COMPILE_STATUS) ? shader : null;
  };
  const vertex = compile(gl.VERTEX_SHADER, VERT);
  const fragment = compile(gl.FRAGMENT_SHADER, FRAG);
  const program = gl.createProgram();
  const buffer = gl.createBuffer();
  const destroy = () => {
    shaders.forEach((shader) => gl.deleteShader(shader));
    gl.deleteProgram(program);
    gl.deleteBuffer(buffer);
  };
  if (!vertex || !fragment || !program || !buffer) { destroy(); return null; }
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) { destroy(); return null; }
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, "a_pos");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
  const time = gl.getUniformLocation(program, "u_time");
  const resolution = gl.getUniformLocation(program, "u_resolution");
  const styles = getComputedStyle(button);
  gl.uniform3fv(gl.getUniformLocation(program, "u_white"), tokenColor(styles, "--color-theme-fg", [0.93, 0.93, 0.93]));
  gl.uniform3fv(gl.getUniformLocation(program, "u_color"), tokenColor(styles, "--color-theme-fg-04", [0.47, 0.47, 0.47]));
  const resize = () => {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.max(1, Math.round(button.clientWidth * dpr));
    canvas.height = Math.max(1, Math.round(button.clientHeight * dpr));
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(resolution, canvas.width, canvas.height);
  };
  resize();
  return {
    resize, destroy,
    draw: (elapsed) => {
      gl.uniform1f(time, elapsed);
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    },
  };
}

export function DownloadFluid() {
  const canvasRef = useRef(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    const button = canvas.parentElement;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let renderer = null;
    let failed = false;
    let lost = false;
    let inView = false;
    let frame = 0;
    let elapsed = 0;
    let lastTime = null;
    const stop = () => {
      cancelAnimationFrame(frame);
      frame = 0;
      lastTime = null;
    };
    const draw = (now) => {
      if (lastTime !== null) elapsed += Math.min((now - lastTime) / 1000, 0.1);
      lastTime = now;
      renderer.draw(motion.matches ? 0 : elapsed);
      if (canvas.dataset.ready !== "true") canvas.dataset.ready = "true";
      if (!motion.matches) frame = requestAnimationFrame(draw);
    };
    const refresh = () => {
      stop();
      if (lost || failed || document.hidden || !inView || !button.matches(":hover, :focus-visible")) return;
      if (!renderer) renderer = createRenderer(canvas, button);
      if (!renderer) { failed = true; return; }
      draw(performance.now());
    };
    const resize = new ResizeObserver(() => { renderer?.resize(); refresh(); });
    resize.observe(button);
    const visibility = new IntersectionObserver(([entry]) => { inView = entry.isIntersecting; refresh(); });
    visibility.observe(button);
    const contextLost = (event) => { event.preventDefault(); lost = true; delete canvas.dataset.ready; stop(); };
    const contextRestored = () => { renderer = null; failed = false; lost = false; refresh(); };
    for (const event of ["pointerenter", "pointerleave", "focus", "blur"]) button.addEventListener(event, refresh);
    document.addEventListener("visibilitychange", refresh);
    motion.addEventListener("change", refresh);
    canvas.addEventListener("webglcontextlost", contextLost);
    canvas.addEventListener("webglcontextrestored", contextRestored);
    return () => {
      stop();
      resize.disconnect();
      visibility.disconnect();
      for (const event of ["pointerenter", "pointerleave", "focus", "blur"]) button.removeEventListener(event, refresh);
      document.removeEventListener("visibilitychange", refresh);
      motion.removeEventListener("change", refresh);
      canvas.removeEventListener("webglcontextlost", contextLost);
      canvas.removeEventListener("webglcontextrestored", contextRestored);
      renderer?.destroy();
    };
  }, []);

  return <canvas ref={canvasRef} className="download-fluid" aria-hidden="true" />;
}
