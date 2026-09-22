import { useEffect, useRef } from "react";

// Apply scroll deltas so a manual seek becomes the new starting position.
export function useScrollSeek(element, onSeek) {
  const callback = useRef(onSeek);
  useEffect(() => { callback.current = onSeek; });

  useEffect(() => {
    let previousY = window.scrollY;
    let frame = 0;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    function update() {
      frame = 0;
      const currentY = window.scrollY;
      const bounds = element.current?.getBoundingClientRect();
      if (bounds && !reducedMotion.matches) {
        const header = document.querySelector(".site-header")?.getBoundingClientRect().height ?? 0;
        const start = Math.max(0, bounds.top + currentY - window.innerHeight * 0.7);
        const end = bounds.bottom + currentY - header;
        const clamp = (position) => Math.max(start, Math.min(end, position));
        const delta = end > start ? (clamp(currentY) - clamp(previousY)) / (end - start) : 0;
        if (delta) callback.current(delta);
      }
      previousY = currentY;
    }
    function scroll() {
      if (!frame) frame = window.requestAnimationFrame(update);
    }
    function resize() { previousY = window.scrollY; }
    window.addEventListener("scroll", scroll, { passive: true });
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("scroll", scroll);
      window.removeEventListener("resize", resize);
      window.cancelAnimationFrame(frame);
    };
  }, [element]);
}
