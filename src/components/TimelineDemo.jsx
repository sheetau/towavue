import { useRef, useState } from "react";
import { useEntranceAnimation } from "./useEntranceAnimation";
import { asset } from "../site/config.mjs";
import { clamp, MIN_SELECTION, updateSelection } from "../site/timeline.mjs";
import media from "../site/gallery.json";

const percent = (value) => Math.round(value * 100);

export function TimelineDemo({ content }) {
  const [selection, setSelection] = useState([0.408, 0.657]);
  const [position, setPosition] = useState(0.46);
  const track = useRef(null);
  const drag = useRef(null);
  const [start, end] = selection;
  useEntranceAnimation(track, (timing) => [
    track.current.querySelector(".timeline-selection").animate([
      { width: `${MIN_SELECTION * 100}%` },
      { width: `${(end - start) * 100}%` },
    ], timing),
    track.current.querySelector('[data-control="end"]').animate([
      { left: `${(start + MIN_SELECTION) * 100}%` }, { left: `${end * 100}%` },
    ], timing),
  ]);
  const atPointer = (event) => {
    const rect = track.current.getBoundingClientRect();
    return clamp((event.clientX - rect.left) / rect.width);
  };

  function pointerDown(event) {
    if (![0, 2].includes(event.button) || drag.current) return;
    const control = event.target.closest("[data-control]");
    let mode = control?.dataset.control ?? "position";
    // A left click seeks anywhere, including within the selection. Right drag
    // moves the selection; touch can drag the shaded region directly.
    if (mode === "selection" && event.button !== 2 && event.pointerType !== "touch") mode = "position";
    if (event.button === 2 && mode !== "selection") return;
    event.preventDefault();
    const point = atPointer(event);
    drag.current = { mode, point, selection, pointerId: event.pointerId };
    event.currentTarget.setPointerCapture(event.pointerId);
    (mode === "position" ? track.current.querySelector('[data-control="position"]') : control)?.focus({ preventScroll: true });
    if (mode === "position") setPosition(point);
    else if (mode !== "selection") setSelection(updateSelection(selection, mode, point));
  }

  function pointerMove(event) {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId) return;
    const point = atPointer(event);
    if (current.mode === "position") setPosition(point);
    else setSelection(updateSelection(current.selection, current.mode, current.mode === "selection" ? current.selection[0] + point - current.point : point));
  }

  function keyDown(event, mode) {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const value = mode === "position" ? position : mode === "end" ? end : start;
    const step = event.shiftKey ? 0.1 : 0.01;
    const next = event.key === "Home" ? 0 : event.key === "End" ? 1 : value + (["ArrowRight", "ArrowUp"].includes(event.key) ? step : -step);
    if (mode === "position") setPosition(clamp(next));
    else setSelection(updateSelection(selection, mode, next));
  }

  const slider = (mode, label, value, min = 0, max = 1) => ({
    role: "slider", tabIndex: 0, "aria-label": label, "aria-orientation": "horizontal",
    "aria-valuemin": percent(min), "aria-valuemax": percent(max), "aria-valuenow": percent(value),
    "aria-valuetext": `${percent(value)}%`, "aria-describedby": "timeline-instructions",
    "data-control": mode, onKeyDown: (event) => keyDown(event, mode),
  });

  return <div className="timeline-demo">
    <p id="timeline-instructions" className="sr-only">{content.instructions}</p>
    <div ref={track} className="timeline-track" onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }} onLostPointerCapture={() => { drag.current = null; }} onContextMenu={(event) => event.preventDefault()}>
      <img className="timeline-wave" src={asset(media.artwork["audio wave"].src)} width={media.artwork["audio wave"].width} height={media.artwork["audio wave"].height} alt="" loading="lazy" draggable="false" />
      <div className="timeline-selection" style={{ left: `${start * 100}%`, width: `${(end - start) * 100}%` }} {...slider("selection", content.selection, start, 0, 1 - (end - start))} />
      <div className="timeline-handle" style={{ left: `${start * 100}%` }} {...slider("start", content.start, start, 0, end - MIN_SELECTION)} />
      <div className="timeline-handle" style={{ left: `${end * 100}%` }} {...slider("end", content.end, end, start + MIN_SELECTION)} />
      <div className="timeline-cti" style={{ left: `${position * 100}%` }} {...slider("position", content.position, position)} />
    </div>
  </div>;
}
