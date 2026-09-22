import { useEffect, useRef, useState } from "react";
import { asset } from "../site/config.mjs";
import { photos, tracks, sampleVideo, formatSize, formatTime } from "../site/media";
import { advanceAudio, createAudioState, toggleShuffle } from "../site/audio-playback.mjs";
import { Icon, Logo, TabClose, WindowControls } from "./Icons";
import { SeekBar } from "./SeekBar";
import { useScrollSeek } from "./useScrollSeek";

const mediaTypes = ["video", "image", "audio"];

export function HeroDemo({ content }) {
  const [tab, setTab] = useState("video");
  const [photo, setPhoto] = useState(0);
  const [audio, setAudio] = useState(() => createAudioState(tracks.length));
  const [videoTime, setVideoTime] = useState(0);
  const [videoPlaying, setVideoPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [videoError, setVideoError] = useState(false);
  const [imageError, setImageError] = useState(false);
  const demo = useRef(null);
  const scrubbing = useRef(false);
  const video = useRef(null);
  const tabRefs = useRef([]);
  const image = photos[photo];
  const track = tracks[audio.track];
  const names = { image: image.name, video: sampleVideo.name, audio: track.name };
  const value = tab === "image" ? photo : tab === "video" ? videoTime : audio.time;
  const max = tab === "image" ? photos.length - 1 : tab === "video" ? duration : track.duration;
  const playing = tab === "video" ? videoPlaying : audio.playing;
  const metadata = tab === "image"
    ? ["Fit", formatSize(image.bytes), "JPG", `${image.width}×${image.height}`, "Smooth"]
    : tab === "video"
      ? [formatSize(sampleVideo.bytes), "MP4", `${sampleVideo.width}×${sampleVideo.height}`, `${sampleVideo.fps} fps`, "1×"]
      : [track.size, track.format, track.sampleRate, track.quality, "Stereo", "1×"];

  useScrollSeek(demo, (delta) => {
    if (scrubbing.current) return;
    const clamp = (position, limit) => Math.max(0, Math.min(limit, position));
    if (tab === "video" && video.current && duration && !videoError && video.current.paused) {
      const next = clamp(video.current.currentTime + delta * duration, duration);
      video.current.currentTime = next;
      setVideoTime(next);
    }
  });

  // A cached default video can load before React attaches its event handlers.
  useEffect(() => {
    const player = video.current;
    if (player?.error) setVideoError(true);
    if (player?.readyState >= 1 && Number.isFinite(player.duration)) setDuration(player.duration);
  }, []);

  useEffect(() => {
    for (const index of [photo - 1, photo + 1]) {
      if (index < 0 || index >= photos.length) continue;
      const next = new Image();
      next.src = asset(`samples/photo-${index + 1}.webp`);
    }
  }, [photo]);

  function seekPreview(fraction) {
    if (tab === "image") {
      const index = Math.round(fraction * (photos.length - 1));
      return { image: asset(`samples/photo-${index + 1}-thumb.webp`), label: `${index + 1} / ${photos.length}` };
    }
    if (tab === "video") {
      const time = fraction * duration;
      const frame = Math.min(sampleVideo.previewCount - 1, Math.floor(time / sampleVideo.previewInterval));
      return { frame, label: formatTime(time) };
    }
    return null;
  }

  useEffect(() => {
    if (tab !== "audio" || !audio.playing) return;
    let previous = performance.now();
    const timer = window.setInterval(() => {
      const now = performance.now();
      const elapsed = (now - previous) / 1000;
      previous = now;
      setAudio((state) => advanceAudio(state, elapsed, tracks));
    }, 100);
    return () => window.clearInterval(timer);
  }, [tab, audio.playing]);

  useEffect(() => {
    const pause = () => {
      if (!document.hidden) return;
      video.current?.pause();
      setAudio((state) => ({ ...state, playing: false }));
    };
    document.addEventListener("visibilitychange", pause);
    return () => document.removeEventListener("visibilitychange", pause);
  }, []);

  function selectTab(type) {
    if (type === tab) return;
    setTab(type);
    video.current?.pause();
    setAudio((state) => ({ ...state, playing: false }));
  }

  async function togglePlayback() {
    if (tab === "audio") {
      setAudio((state) => ({ ...state, time: state.time >= tracks[state.track].duration ? 0 : state.time, playing: !state.playing }));
    } else if (tab === "video" && video.current && duration && !videoError) {
      const player = video.current;
      if (!player.paused) player.pause();
      else {
        if (player.ended) player.currentTime = 0;
        try { await player.play(); } catch { setVideoPlaying(false); }
      }
    }
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
    } else setAudio((state) => ({ ...state, time: position }));
  }

  return (
    <figure ref={demo} className="hero-mockup" aria-label={content.label}>
      <div className="demo-window">
        <div className="demo-toolbar">
          <span className="demo-logo-slot"><Logo className="demo-logo" /></span>
          <div className="demo-tabs" role="tablist" aria-label={content.tabs}>
            {mediaTypes.map((type, index) => (
              <button key={type} type="button" className="demo-tab" role="tab" id={`demo-tab-${type}`} aria-controls={`demo-panel-${type}`} aria-selected={tab === type} aria-label={content[type]} tabIndex={tab === type ? 0 : -1} onClick={() => selectTab(type)} onKeyDown={(event) => tabKey(event, index)} ref={(element) => { tabRefs.current[index] = element; }}>
                <span className="demo-tab-name">{names[type]}</span><span className="demo-tab-short">{content[type]}</span><TabClose />
              </button>
            ))}
          </div>
          <WindowControls />
        </div>
        <div className="demo-viewport">
          <div className="demo-panel" id="demo-panel-image" role="tabpanel" aria-labelledby="demo-tab-image" hidden={tab !== "image"} tabIndex={0}>
            <img className="demo-photo" src={asset(`samples/photo-${photo + 1}.webp`)} alt={content.imageAlt[photo]} width={image.width} height={image.height} fetchPriority="high" onError={() => setImageError(true)} />
            {imageError && <p className="demo-error" role="status">{content.error}</p>}
          </div>
          <div className="demo-panel" id="demo-panel-video" role="tabpanel" aria-labelledby="demo-tab-video" hidden={tab !== "video"} tabIndex={0}>
            <video ref={video} src={asset("samples/flowers.mp4")} poster={asset("samples/video-poster.webp")} muted playsInline preload="auto" disablePictureInPicture onLoadedMetadata={(event) => {
              const player = event.currentTarget;
              setDuration(Number.isFinite(player.duration) ? player.duration : 0);
              player.currentTime = Math.min(videoTime, player.duration || 0);
            }} onTimeUpdate={(event) => setVideoTime(event.currentTarget.currentTime)} onPlay={() => setVideoPlaying(true)} onPause={() => setVideoPlaying(false)} onEnded={() => setVideoPlaying(false)} onError={() => { setVideoError(true); setVideoPlaying(false); }} />
            <button className="demo-video-toggle" type="button" aria-label={videoPlaying ? content.pauseVideo : content.playVideo} onClick={togglePlayback} disabled={!duration || videoError} />
            {videoError && <p className="demo-error" role="status">{content.error}</p>}
          </div>
          <div className="demo-panel demo-audio" id="demo-panel-audio" role="tabpanel" aria-labelledby="demo-tab-audio" hidden={tab !== "audio"} tabIndex={0}>
            <ol className="audio-playlist" aria-label={content.playlist}>
              {tracks.map((item, index) => <li key={item.name}><button type="button" aria-pressed={audio.track === index} onClick={() => setAudio((state) => ({ ...state, track: index, time: 0 }))}><span className="track-number">{index + 1}.</span><span className="track-name">{item.name}</span><span className="track-duration">{formatTime(item.duration)}</span></button></li>)}
            </ol>
          </div>
        </div>
        <SeekBar key={tab} label={content[`${tab}Seek`]} valueText={tab === "image" ? `${photo + 1} / ${photos.length}` : `${formatTime(value)} / ${formatTime(max)}`} value={value} max={max} step={tab === "image" ? 1 : 0.01} disabled={tab === "video" && (!duration || videoError)} onChange={seek} getPreview={seekPreview} onScrubbingChange={(active) => { scrubbing.current = active; }} />
        <div className="demo-statusbar">
          <div className="demo-transport">
            {tab === "image" ? <span className="demo-reading" role="img" aria-label={content.reading}><Icon name="book" /></span> : <button className="demo-icon-button" type="button" aria-label={playing ? content.pause : content.play} title={playing ? content.pause : content.play} onClick={togglePlayback} disabled={tab === "video" && (!duration || videoError)}><Icon name={playing ? "pause" : "play"} /></button>}
            {tab === "audio" && <>
              <button className="demo-icon-button" type="button" aria-label={content.repeat[audio.repeat]} title={content.repeat[audio.repeat]} aria-pressed={audio.repeat !== "off"} onClick={() => setAudio((state) => ({ ...state, repeat: { off: "all", all: "one", one: "off" }[state.repeat] }))}><Icon name={audio.repeat === "one" ? "repeat-one" : "repeat"} /></button>
              <button className="demo-icon-button" type="button" aria-label={content.shuffle[audio.shuffle ? "on" : "off"]} title={content.shuffle[audio.shuffle ? "on" : "off"]} aria-pressed={audio.shuffle} onClick={() => setAudio((state) => toggleShuffle(state))}><Icon name="shuffle" /></button>
            </>}
          </div>
          <span className="demo-position">{tab === "image" ? `${photo + 1} / ${photos.length}` : `${formatTime(value)} / ${formatTime(max)}`}</span>
          <span className="demo-path">{tab === "image" ? "Photos" : tab === "video" ? "Videos" : "Music"}\{names[tab]}</span>
          <span className="demo-metadata">{metadata.map((value, index) => <span key={index}>{value}</span>)}</span>
        </div>
      </div>
    </figure>
  );
}
