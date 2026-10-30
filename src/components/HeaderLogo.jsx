import { useEffect, useRef } from "react";
import { CubeLogo } from "./Icons";
import { createLogoCubeRenderer, LOGO_FADE_DURATION, LOGO_IDLE_DELAY, LOGO_SPIN_DURATION, spinAngle } from "../site/logo-cube.mjs";

// Both layers reach 80% opacity at the midpoint of each crossfade.
const FADE_OVERLAP_POWER = Math.log(0.2) / Math.log(0.5);

export function HeaderLogo({ href, label }) {
  const linkRef = useRef(null);
  const canvasRef = useRef(null);

  useEffect(() => {
    const link = linkRef.current;
    const canvas = canvasRef.current;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let renderer = null;
    let frame = 0;
    let timer = 0;
    let busy = false;
    let disposed = false;
    let failed = false;
    let lost = false;
    let inView = false;
    let generation = 0;
    let nextDirection = "left";
    const allowed = () => !disposed && !failed && !lost && !document.hidden && inView && !motion.matches;
    const schedule = () => {
      clearTimeout(timer);
      if (allowed()) timer = window.setTimeout(spin, LOGO_IDLE_DELAY);
    };
    const stop = () => {
      generation += 1;
      cancelAnimationFrame(frame);
      clearTimeout(timer);
      busy = false;
      delete link.dataset.spinning;
      delete link.dataset.logoPhase;
      link.style.removeProperty("--logo-cube-opacity");
      link.style.removeProperty("--logo-svg-opacity");
    };
    async function spin() {
      if (busy || !allowed()) return;
      busy = true;
      clearTimeout(timer);
      const current = ++generation;
      if (!renderer) {
        let next = null;
        try { next = await createLogoCubeRenderer(canvas, link.querySelector("svg")); } catch { /* Static SVG fallback. */ }
        if (disposed || current !== generation) { next?.destroy(); return; }
        renderer = next;
        if (!renderer) { failed = true; busy = false; return; }
      }
      if (!allowed()) { stop(); return; }
      const direction = nextDirection;
      const start = performance.now();
      const draw = (now) => {
        const elapsed = now - start;
        const progress = elapsed / LOGO_SPIN_DURATION;
        if (progress >= 1) {
          nextDirection = direction === "left" ? "up" : "left";
          stop();
          schedule();
          return;
        }
        renderer.draw(spinAngle(progress), direction);
        // Overlap the two fades at 80% / 80% to soften the brightness dip
        // of a linear crossfade without holding either layer fully opaque.
        const entering = elapsed < LOGO_FADE_DURATION;
        const exiting = elapsed > LOGO_SPIN_DURATION - LOGO_FADE_DURATION;
        const fade = Math.max(0, Math.min(1, entering
          ? elapsed / LOGO_FADE_DURATION
          : (elapsed - (LOGO_SPIN_DURATION - LOGO_FADE_DURATION)) / LOGO_FADE_DURATION));
        const blend = fade * fade * (3 - 2 * fade);
        const incoming = 1 - (1 - blend) ** FADE_OVERLAP_POWER;
        const outgoing = 1 - blend ** FADE_OVERLAP_POWER;
        link.dataset.logoPhase = exiting ? "exit" : entering ? "enter" : "spin";
        link.style.setProperty("--logo-cube-opacity", String(entering ? incoming : exiting ? outgoing : 1));
        link.style.setProperty("--logo-svg-opacity", String(entering ? outgoing : exiting ? incoming : 0));
        link.dataset.spinning = "true";
        frame = requestAnimationFrame(draw);
      };
      draw(start);
    }
    const refresh = () => { stop(); schedule(); };
    const pointerEnter = (event) => { if (event.pointerType !== "touch") spin(); };
    const focus = () => { if (link.matches(":focus-visible")) spin(); };
    const contextLost = (event) => {
      event.preventDefault();
      lost = true;
      stop();
    };
    const contextRestored = () => {
      renderer?.destroy();
      renderer = null;
      lost = failed = false;
      refresh();
    };
    const visibility = new IntersectionObserver(([entry]) => {
      inView = entry.isIntersecting;
      refresh();
    });
    visibility.observe(link);
    link.addEventListener("pointerenter", pointerEnter);
    link.addEventListener("focus", focus);
    document.addEventListener("visibilitychange", refresh);
    motion.addEventListener("change", refresh);
    canvas.addEventListener("webglcontextlost", contextLost);
    canvas.addEventListener("webglcontextrestored", contextRestored);
    return () => {
      disposed = true;
      stop();
      visibility.disconnect();
      link.removeEventListener("pointerenter", pointerEnter);
      link.removeEventListener("focus", focus);
      document.removeEventListener("visibilitychange", refresh);
      motion.removeEventListener("change", refresh);
      canvas.removeEventListener("webglcontextlost", contextLost);
      canvas.removeEventListener("webglcontextrestored", contextRestored);
      renderer?.destroy();
    };
  }, []);

  return <a ref={linkRef} className="site-logo" href={href} aria-label={label}>
    <CubeLogo />
    <canvas ref={canvasRef} className="logo-cube" aria-hidden="true" />
  </a>;
}
