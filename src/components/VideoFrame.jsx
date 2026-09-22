import { asset } from "../site/config.mjs";
import { sampleVideo } from "../site/media";

export function VideoFrame({ frame, className, highResolution = false }) {
  const count = sampleVideo.previewCount;
  const index = Math.max(0, Math.min(count - 1, frame));
  const width = highResolution ? sampleVideo.frameWidth : sampleVideo.previewWidth;
  const height = highResolution ? sampleVideo.frameHeight : sampleVideo.previewHeight;
  return <span className={className} style={{
    aspectRatio: `${width} / ${height}`,
    backgroundImage: `url("${asset(highResolution ? "samples/video-frames.webp" : "samples/video-thumbnails.webp")}")`,
    backgroundSize: `${count * 100}% 100%`,
    backgroundPosition: `${count > 1 ? index / (count - 1) * 100 : 0}% 0`,
  }} />;
}
