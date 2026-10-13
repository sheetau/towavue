import { useEffect, useRef } from "react";
import { Logo, TabClose, WindowControls } from "./Icons";
import { MediaThumbnail } from "./MediaThumbnail";
import media from "../site/gallery.json";

export function HeroDemo({ content }) {
  const viewport = useRef(null);
  useEffect(() => {
    const element = viewport.current;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    const update = () => {
      frame = 0;
      const max = Math.max(0, element.scrollHeight - element.clientHeight);
      const pageTop = element.getBoundingClientRect().top + window.scrollY;
      // Traverse every row while the gallery is still on screen, at any breakpoint.
      const start = Math.max(0, pageTop - window.innerHeight * 0.65);
      const end = Math.max(start + 1, pageTop + element.clientHeight * 0.2);
      const progress = Math.min(1, Math.max(0, (window.scrollY - start) / (end - start)));
      element.scrollTop = motion.matches ? 0 : max * progress;
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(update); };
    const resize = new ResizeObserver(schedule);
    resize.observe(element);
    resize.observe(element.firstElementChild);
    window.addEventListener("resize", schedule);
    window.addEventListener("scroll", schedule, { passive: true });
    motion.addEventListener("change", schedule);
    update();
    return () => { cancelAnimationFrame(frame); resize.disconnect(); window.removeEventListener("resize", schedule); window.removeEventListener("scroll", schedule); motion.removeEventListener("change", schedule); };
  }, []);

  return <figure className="hero-mockup" aria-label={content.label}>
    <div className="demo-window">
      <div className="demo-toolbar" aria-hidden="true">
        <span className="demo-logo-slot"><Logo /></span>
        <div className="demo-tab"><span>{content.gallery}</span><TabClose /></div>
        <WindowControls />
      </div>
      <div className="gallery-viewport" ref={viewport}>
        <div className="gallery-grid">
          {media.photos.map((photo, index) => <div className="gallery-cell" key={photo.name}><MediaThumbnail photo={photo} loading={index < 5 ? "eager" : "lazy"} /></div>)}
        </div>
      </div>
      <figcaption className="demo-statusbar">{content.date}</figcaption>
    </div>
  </figure>;
}
