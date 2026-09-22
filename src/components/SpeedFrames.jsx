import { sampleVideo } from "../site/media";
import { VideoFrame } from "./VideoFrame";

export function SpeedFrames({ label }) {
  const layers = sampleVideo.previewCount;
  // Account for the CSS skew/scale so the visible planes, not their untransformed
  // boxes, fit exactly between the stage's top and bottom padding.
  const ratio = sampleVideo.frameWidth / sampleVideo.frameHeight;
  const angleX = 24;
  const angleY = -10;
  const skewX = Math.tan(angleX * Math.PI / 180);
  const skewY = Math.tan(angleY * Math.PI / 180);
  const scaleY = 0.62;
  const projectedWidth = ratio + skewX * scaleY;
  const projectedHeight = Math.abs(skewY) * ratio + (1 + skewX * skewY) * scaleY;
  return <div className="speed-frame-stack" role="img" aria-label={label} style={{
    "--frame-aspect": projectedWidth / projectedHeight,
    "--frame-height": `${100 / projectedHeight}%`,
    "--frame-top": `${Math.abs(skewY) * ratio / projectedHeight * 100}%`,
    "--frame-skew-x": `${angleX}deg`,
    "--frame-skew-y": `${angleY}deg`,
    "--frame-scale-y": scaleY,
  }}>
    {Array.from({ length: layers }, (_, index) => <div key={index} className="speed-frame-layer" style={{ "--frame-position": `${index / (layers - 1) * 100}%`, "--frame-order": layers - 1 - index, "--frame-brightness": 1 - index / (layers - 1) * 0.75, zIndex: layers - index }}>
      <VideoFrame className="speed-frame" frame={layers - 1 - index} highResolution />
    </div>)}
  </div>;
}
