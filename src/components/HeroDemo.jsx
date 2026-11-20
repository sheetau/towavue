import { useEffect, useRef } from "react";
import { CubeLogo, TabClose, WindowControls } from "./Icons";
import { MediaThumbnail } from "./MediaThumbnail";

export function HeroDemo({ content, photos }) {
  const viewport = useRef(null);
  useEffect(() => {
    const element = viewport.current;
    const demo = element.closest("figure");
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    let needsMeasure = true;
    let start = 0;
    let end = 0;
    let travel = 0;
    let lastOffset = -1;

    const update = () => {
      frame = 0;
      const pageY = window.scrollY;
      if (needsMeasure) {
        const bounds = demo.getBoundingClientRect();
        start = Math.max(0, pageY + bounds.top - window.innerHeight * 0.65);
        // The demo leaves the screen when its bottom crosses the viewport top.
        end = pageY + bounds.bottom;
        travel = Math.max(0, element.scrollHeight - element.clientHeight);
        needsMeasure = false;
        lastOffset = -1;
      }
      const progress = Math.min(1, Math.max(0, (pageY - start) / Math.max(1, end - start)));
      const offset = motion.matches ? 0 : Math.round(travel * progress);
      if (offset !== lastOffset) {
        element.scrollTop = offset;
        lastOffset = offset;
      }
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    const invalidate = () => {
      needsMeasure = true;
      schedule();
    };
    const resize = new ResizeObserver(invalidate);
    resize.observe(demo.parentElement); // Includes the heading and buttons above the demo.
    resize.observe(element);
    resize.observe(element.firstElementChild);
    window.addEventListener("resize", invalidate);
    window.addEventListener("scroll", schedule, { passive: true });
    motion.addEventListener("change", schedule);
    update();
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
      window.removeEventListener("resize", invalidate);
      window.removeEventListener("scroll", schedule);
      motion.removeEventListener("change", schedule);
    };
  }, []);

  return <figure className="hero-mockup" aria-label={content.label}>
    <div className="demo-window">
      <div className="demo-toolbar" aria-hidden="true">
        <span className="demo-logo-slot"><CubeLogo /></span>
        <div className="demo-tab"><span>{content.gallery}</span><TabClose /></div>
        <WindowControls />
      </div>
      <div className="gallery-viewport" ref={viewport}>
        <div className="gallery-grid">
          {photos.map((photo, index) => <div className="gallery-cell" key={photo.name}><MediaThumbnail photo={photo} loading={index < 5 ? "eager" : "lazy"} /></div>)}
        </div>
      </div>
      <figcaption className="demo-statusbar">{content.date}</figcaption>
    </div>
  </figure>;
}
