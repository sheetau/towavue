import { useState } from "react";

export function SeekBar({ label, valueText, value, max, step, disabled, onChange }) {
  const [hover, setHover] = useState(null);
  const progress = max ? value / max * 100 : 0;

  function preview(event) {
    if (event.pointerType === "touch" || disabled) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    setHover(Math.max(0, Math.min(100, (event.clientX - bounds.left) / bounds.width * 100)));
  }

  return <div className={`demo-seekbar${hover !== null && !disabled ? " is-hovered" : ""}${disabled ? " is-disabled" : ""}`} style={{ "--seek-progress": `${progress}%`, "--seek-hover": `${hover ?? 0}%` }}>
    <div className="demo-seek-track" aria-hidden="true"><span className="demo-seek-progress" /><span className="demo-seek-preview" /></div>
    <span className="demo-seek-thumb" aria-hidden="true" />
    <input type="range" aria-label={label} aria-valuetext={valueText} min="0" max={max || 1} step={step} value={value} disabled={disabled} onChange={onChange} onPointerEnter={preview} onPointerMove={preview} onPointerLeave={() => setHover(null)} onPointerCancel={() => setHover(null)} />
  </div>;
}
