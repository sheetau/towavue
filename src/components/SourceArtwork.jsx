import { asset } from "../site/config.mjs";

export function SourceArtwork({ label }) {
  return <div className="source-artwork" role="img" aria-label={label}>
    <img className="source-code" src={asset("code.webp")} alt="" loading="lazy" />
    <img className="source-window" src={asset("image3.webp")} alt="" loading="lazy" />
  </div>;
}
