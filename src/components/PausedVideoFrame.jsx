import { useEffect, useState } from "react";
import { asset } from "../site/config.mjs";
import { sampleVideo } from "../site/media";

export function PausedVideoFrame({ time, active, visible }) {
  const [source, setSource] = useState(asset("samples/video-poster.webp"));
  const frame = Math.min(sampleVideo.stillFrameCount - 1, Math.max(0, Math.floor(time * sampleVideo.stillFrameRate)));

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const image = new Image();
    const url = asset(`samples/video-stills/frame-${String(frame).padStart(3, "0")}.webp`);
    image.src = url;
    // Keep the last decoded image visible until the requested frame is ready.
    image.decode().then(() => {
      if (!cancelled) setSource(url);
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [active, frame]);

  return <img className="demo-paused-frame" src={source} alt="" aria-hidden="true" hidden={!visible} width={sampleVideo.width} height={sampleVideo.height} />;
}
