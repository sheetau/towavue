import { useRef, useState } from "react";
import { asset } from "../site/config.mjs";
import { photos, tracks, formatTime } from "../site/media";
import { Icon, Logo } from "./Icons";

const mediaTypes = ["image", "video", "audio"];

export function HeroDemo({ content }) {
  const [tab, setTab] = useState("image");
  const [photo, setPhoto] = useState(0);
  const [track, setTrack] = useState(0);
  const [videoTime, setVideoTime] = useState(0);
  const [audioTime, setAudioTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [videoError, setVideoError] = useState(false);
  const [imageError, setImageError] = useState(false);
  const video = useRef(null);
  const tabRefs = useRef([]);
  const names = { image: photos[photo], video: "Monochaetum.mp4", audio: tracks[track].name };
  const value = tab === "image" ? photo : tab === "video" ? videoTime : audioTime;
  const max = tab === "image" ? photos.length - 1 : tab === "video" ? duration : tracks[track].duration;

  function selectTab(type) {
    setTab(type);
    video.current?.pause();
  }

  function tabKey(event, index) {
    const next = event.key === "ArrowRight" ? (index + 1) % 3 : event.key === "ArrowLeft" ? (index + 2) % 3 : event.key === "Home" ? 0 : event.key === "End" ? 2 : null;
    if (next === null) return;
    event.preventDefault();
    selectTab(mediaTypes[next]);
    tabRefs.current[next]?.focus();
  }

  function seek(event) {
    const position = Number(event.target.value);
    if (tab === "image") { setPhoto(position); setImageError(false); }
    else if (tab === "video") {
      if (video.current && duration) video.current.currentTime = position;
      setVideoTime(position);
    } else setAudioTime(position);
  }

  return (
    <figure className="hero-mockup" aria-label={content.label}>
      <div className="demo-window">
        <div className="demo-toolbar">
          <Logo className="demo-logo" />
          <div className="demo-tabs" role="tablist" aria-label={content.tabs}>
            {mediaTypes.map((type, index) => (
              <button key={type} type="button" className="demo-tab" role="tab" id={`demo-tab-${type}`} aria-controls={`demo-panel-${type}`} aria-selected={tab === type} aria-label={content[type]} tabIndex={tab === type ? 0 : -1} onClick={() => selectTab(type)} onKeyDown={(event) => tabKey(event, index)} ref={(element) => { tabRefs.current[index] = element; }}>
                <img className="demo-tab-favicon" src={asset("favicon.ico")} alt="" width="12" height="12" />
                <span className="demo-tab-name">{names[type]}</span><span className="demo-tab-short">{content[type]}</span><Icon name="close" className="demo-tab-close" />
              </button>
            ))}
          </div>
          <div className="demo-window-controls" aria-hidden="true"><span>−</span><span>□</span><Icon name="close" /></div>
        </div>
        <div className="demo-viewport">
          <div className="demo-panel" id="demo-panel-image" role="tabpanel" aria-labelledby="demo-tab-image" hidden={tab !== "image"} tabIndex={0}>
            <img className="demo-photo" src={asset(`samples/photo-${photo + 1}.webp`)} alt={content.imageAlt[photo]} width="1440" height="960" fetchPriority="high" onError={() => setImageError(true)} />
            {imageError && <p className="demo-error" role="status">{content.error}</p>}
          </div>
          <div className="demo-panel" id="demo-panel-video" role="tabpanel" aria-labelledby="demo-tab-video" hidden={tab !== "video"} tabIndex={0}>
            {tab === "video" && <video ref={video} src={asset("samples/flowers.mp4")} poster={asset("samples/video-poster.webp")} muted playsInline preload="auto" disablePictureInPicture onLoadedMetadata={(event) => {
              const player = event.currentTarget;
              setDuration(Number.isFinite(player.duration) ? player.duration : 0);
              player.currentTime = Math.min(videoTime, player.duration || 0);
            }} onTimeUpdate={(event) => setVideoTime(event.currentTarget.currentTime)} onError={() => setVideoError(true)} />}
            <span className="demo-silent"><Icon name="muted" />{content.silent}</span>
            {videoError && <p className="demo-error" role="status">{content.error}</p>}
          </div>
          <div className="demo-panel demo-audio" id="demo-panel-audio" role="tabpanel" aria-labelledby="demo-tab-audio" hidden={tab !== "audio"} tabIndex={0}>
            <div className="audio-now-playing" aria-live="polite"><Icon name="audio" /><span>{tracks[track].name}</span><span className="audio-silent">{content.silent}</span></div>
            <ol className="audio-playlist" aria-label={content.playlist}>
              {tracks.map((item, index) => <li key={item.name}><button type="button" aria-pressed={track === index} onClick={() => { setTrack(index); setAudioTime(0); }}><span className="track-number">{index + 1}.</span><span className="track-name">{item.name}</span><span className="track-duration">{formatTime(item.duration)}</span></button></li>)}
            </ol>
          </div>
        </div>
        <div className="demo-seekbar">
          <input type="range" aria-label={content[`${tab}Seek`]} aria-valuetext={tab === "image" ? `${photo + 1} / ${photos.length}` : `${formatTime(value)} / ${formatTime(max)}`} min="0" max={max || 1} step={tab === "image" ? 1 : 0.01} value={value} disabled={tab === "video" && (!duration || videoError)} onChange={seek} style={{ "--seek-progress": `${max ? value / max * 100 : 0}%` }} />
        </div>
        <div className="demo-statusbar">
          <Icon name={tab === "image" ? "book" : "video"} />
          <span className="demo-position">{tab === "image" ? `${photo + 1} / ${photos.length}` : `${formatTime(value)} / ${formatTime(max)}`}</span>
          <span className="demo-path">{tab === "image" ? "Photos" : tab === "video" ? "Videos" : "Music"}\{names[tab]}</span>
          <span className="demo-metadata">{tab === "image" ? "Fit　 JPG　 Smooth" : tab === "video" ? "MP4　 1×" : "1×"}</span>
        </div>
      </div>
      <figcaption>{tab === "audio" ? content.audioHint : content.hint}</figcaption>
    </figure>
  );
}
