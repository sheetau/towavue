import { useLayoutEffect, useRef, useState } from "react";
import { useEntranceAnimation } from "./useEntranceAnimation";
import media from "../site/gallery.json";
import { MediaThumbnail } from "./MediaThumbnail";

const photos = media.filmstripIds.map((id) => media.photos.find((photo) => photo.id === id));

export function FilmstripDemo({ content }) {
  const [active, setActive] = useState(1);
  const [bounds, setBounds] = useState(null);
  const strip = useRef(null);
  const items = useRef([]);
  const keyboardNavigation = useRef(false);

  useLayoutEffect(() => {
    const measure = () => {
      const root = strip.current;
      const image = items.current[active]?.querySelector("img");
      if (!root || !image) return;
      const frame = root.getBoundingClientRect();
      const rect = image.getBoundingClientRect();
      const left = rect.left - frame.left + root.scrollLeft - 10;
      const contentWidth = root.querySelector(".filmstrip-items").offsetWidth;
      setBounds({ left, top: rect.top - frame.top - 10, width: rect.width + 20, height: rect.height + 20, "--caption-width": `${Math.min(170, contentWidth - left - 4)}px` });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(strip.current);
    return () => observer.disconnect();
  }, [active]);

  const finishEntrance = useEntranceAnimation(strip, (timing) => {
    const offset = Math.max(0, strip.current.clientWidth + strip.current.scrollLeft - bounds.left - bounds.width - 12);
    return [strip.current.querySelector(".filmstrip-indicator").animate([
      { transform: `translateX(${offset}px)` }, { transform: "translateX(0)" },
    ], timing)];
  }, { ready: Boolean(bounds), layoutKey: bounds ? `${bounds.left}:${bounds.width}` : "" });

  function choose(index) {
    finishEntrance();
    setActive(index);
  }

  function navigate(event, index) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    keyboardNavigation.current = true;
    const next = event.key === "Home" ? 0 : event.key === "End" ? photos.length - 1 : Math.min(photos.length - 1, Math.max(0, index + (event.key === "ArrowRight" ? 1 : -1)));
    items.current[next]?.focus({ preventScroll: true });
    items.current[next]?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
  }

  return <div className="filmstrip-demo" role="group" aria-label={content.label}>
    <div className="filmstrip-scroll" ref={strip} data-lenis-prevent-horizontal="">
      <div className="filmstrip-items">
        {photos.map((photo, index) => <div className="filmstrip-cell" key={photo.name}>
          <button ref={(element) => { items.current[index] = element; }} className="filmstrip-item" type="button" aria-pressed={active === index} aria-label={`${index + 1} / ${photos.length}: ${photo.name}`} onPointerEnter={() => { if (!keyboardNavigation.current) choose(index); }} onPointerMove={() => { keyboardNavigation.current = false; choose(index); }} onFocus={() => choose(index)} onClick={() => { keyboardNavigation.current = false; choose(index); }} onKeyDown={(event) => navigate(event, index)}>
            <MediaThumbnail photo={photo} loading="lazy" />
          </button>
        </div>)}
      </div>
      {bounds && <div className="filmstrip-indicator" style={bounds} aria-hidden="true">
        <i /><i /><i /><i />
        <span className="filmstrip-index">{active + 1} / {photos.length}</span>
        <span className="filmstrip-filename">{photos[active].name}</span>
      </div>}
    </div>
  </div>;
}
