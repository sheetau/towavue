import { useCallback, useEffect, useRef } from "react";

// Keep the actual control values intact: the entrance is only a visual overlay.
export function useEntranceAnimation(target, animate, { ready = true, layoutKey = "" } = {}) {
  const factory = useRef(animate);
  const played = useRef(false);
  const finish = useRef(null);
  factory.current = animate;

  const finishEntrance = useCallback(() => {
    played.current = true;
    finish.current?.();
  }, []);

  useEffect(() => {
    const element = target.current;
    if (!ready || !element || played.current) return;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (motion.matches || !element.animate || !("IntersectionObserver" in window)) {
      played.current = true;
      element.dataset.entrance = "complete";
      return;
    }

    const animations = factory.current({
      duration: 950,
      easing: getComputedStyle(document.documentElement).getPropertyValue("--ease").trim(),
      fill: "both",
    });
    animations.forEach((animation) => {
      animation.pause();
      animation.currentTime = 0;
    });
    element.dataset.entrance = "waiting";
    let remaining = animations.length;
    animations.forEach((animation) => {
      animation.onfinish = () => {
        animation.cancel();
        if (--remaining === 0) element.dataset.entrance = "complete";
      };
    });
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting && entry.intersectionRatio >= 0.85)) return;
      played.current = true;
      element.dataset.entrance = "running";
      observer.disconnect();
      animations.forEach((animation) => animation.play());
    }, { threshold: 0.85, rootMargin: "0px 0px -64px 0px" });
    observer.observe(element);

    const complete = () => {
      observer.disconnect();
      animations.forEach((animation) => animation.cancel());
      element.dataset.entrance = "complete";
    };
    finish.current = complete;
    element.addEventListener("pointerdown", finishEntrance, true);
    element.addEventListener("keydown", finishEntrance, true);
    element.addEventListener("focusin", finishEntrance, true);
    motion.addEventListener("change", finishEntrance);
    return () => {
      complete();
      finish.current = null;
      element.removeEventListener("pointerdown", finishEntrance, true);
      element.removeEventListener("keydown", finishEntrance, true);
      element.removeEventListener("focusin", finishEntrance, true);
      motion.removeEventListener("change", finishEntrance);
    };
  }, [target, ready, layoutKey, finishEntrance]);

  return finishEntrance;
}
