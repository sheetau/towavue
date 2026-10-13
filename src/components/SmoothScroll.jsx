import { useEffect } from "react";
import Lenis from "lenis";

export function SmoothScroll() {
  useEffect(() => {
    // Match Pixel Reveal's Lenis interpolation without adding a second easing
    // layer to the gallery or the page-progress indicator.
    const scroll = new Lenis({
      autoRaf: true,
      lerp: 0.1,
      smoothWheel: true,
      syncTouch: false,
      anchors: true,
      stopInertiaOnNavigate: true,
      respectReducedMotion: true,
      // Preserve Shift+wheel for native horizontal filmstrip scrolling.
      virtualScroll: ({ event }) => !event.shiftKey,
    });
    return () => scroll.destroy();
  }, []);

  return null;
}
