import { useEffect, useRef } from "react";

export function AmbientFish() {
  const canvasRef = useRef(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let renderer = null;
    let observer = null;
    let frame = 0;
    let last = null;
    let disposed = false;
    const stop = () => { cancelAnimationFrame(frame); frame = 0; last = null; };
    const tick = (now) => {
      if (disposed || document.hidden) { stop(); return; }
      if (last === null) last = now;
      const elapsed = now - last;
      // Decorative motion is capped at 30 fps; speed is independent of frames.
      if (elapsed >= 1000 / 30 - 0.5) {
        renderer.draw(Math.min(elapsed / 1000, 0.08));
        last = now;
      }
      frame = requestAnimationFrame(tick);
    };
    const refresh = () => {
      stop();
      if (!renderer || document.hidden || disposed) return;
      renderer.draw(0);
      canvas.dataset.ready = "true";
      if (!motion.matches) frame = requestAnimationFrame(tick);
    };
    // Defer the decorative renderer so font loading and content get priority.
    const start = () => {
      import("../site/ambient-fish.mjs").then(({ createAmbientFish }) => {
        if (disposed) return;
        renderer = createAmbientFish(canvas);
        if (!renderer) return;
        observer = new ResizeObserver(() => { renderer.resize(); refresh(); });
        observer.observe(canvas);
        renderer.resize();
        refresh();
      }).catch(() => { /* The original black background is the fallback. */ });
    };
    const idle = window.requestIdleCallback?.(start, { timeout: 1600 });
    const timeout = idle === undefined ? window.setTimeout(start, 600) : null;
    document.addEventListener("visibilitychange", refresh);
    motion.addEventListener("change", refresh);
    return () => {
      disposed = true;
      stop();
      if (idle !== undefined) window.cancelIdleCallback?.(idle);
      if (timeout !== null) window.clearTimeout(timeout);
      observer?.disconnect();
      document.removeEventListener("visibilitychange", refresh);
      motion.removeEventListener("change", refresh);
      renderer?.destroy();
    };
  }, []);
  return <canvas ref={canvasRef} className="ambient-fish" aria-hidden="true" />;
}
