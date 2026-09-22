import { useState } from "react";

export function SeekBar({ label, valueText, value, max, step, disabled, onChange, getPreview, onScrubbingChange }) {
  const [hover, setHover] = useState(null);
  const thumbnail = hover !== null && !disabled ? getPreview?.(hover / 100) : null;
  const progress = max ? value / max * 100 : 0;

  function preview(event) {
    if (event.pointerType === "touch" || disabled) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    setHover(Math.max(0, Math.min(100, (event.clientX - bounds.left) / bounds.width * 100)));
  }

  return <div className={`demo-seekbar${hover !== null && !disabled ? " is-hovered" : ""}${disabled ? " is-disabled" : ""}`} style={{ "--seek-progress": `${progress}%`, "--seek-hover": `${hover ?? 0}%` }}>
    <div className="demo-seek-track" aria-hidden="true"><span className="demo-seek-progress" /><span className="demo-seek-preview" /></div>
    <span className="demo-seek-thumb" aria-hidden="true" />
    {thumbnail && <div className="demo-seek-thumbnail" aria-hidden="true">
      {thumbnail.image ? <img src={thumbnail.image} alt="" /> : <span className="demo-seek-video-frame" style={{ backgroundImage: `url("${thumbnail.sprite}")`, backgroundSize: `${thumbnail.count * 100}% 100%`, backgroundPosition: `${thumbnail.count > 1 ? thumbnail.frame / (thumbnail.count - 1) * 100 : 0}% 0` }} />}
      <span className="demo-seek-thumbnail-label">{thumbnail.label}</span>
    </div>}
    <input type="range" aria-label={label} aria-valuetext={valueText} min="0" max={max || 1} step={step} value={value} disabled={disabled} onChange={onChange} onPointerDown={() => onScrubbingChange?.(true)} onPointerUp={() => onScrubbingChange?.(false)} onLostPointerCapture={() => onScrubbingChange?.(false)} onPointerEnter={preview} onPointerMove={preview} onPointerLeave={() => setHover(null)} onPointerCancel={() => { setHover(null); onScrubbingChange?.(false); }} />
  </div>;
}
