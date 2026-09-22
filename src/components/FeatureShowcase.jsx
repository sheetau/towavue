import { useState } from "react";
import { asset } from "../site/config.mjs";
import { SpeedFrames } from "./SpeedFrames";

export function FeatureShowcase({ content }) {
  const [active, setActive] = useState(0);
  const feature = content.features[active];
  return (
    <div className="feature-showcase">
      <ol className="feature-list" aria-label={content.featureLabel}>
        {content.features.map((feature, index) => (
          <li key={feature.id} className={active === index ? "is-active" : ""}>
            <h4><button type="button" className="feature-select" aria-pressed={active === index} aria-controls="feature-preview" aria-describedby={`feature-description-${feature.id}`} onClick={() => setActive(index)}>
              <span className="feature-number" aria-hidden="true">{String(index + 1).padStart(2, "0")}</span><span>{feature.title}</span>
            </button></h4>
            <p id={`feature-description-${feature.id}`}>{feature.description}</p>
          </li>
        ))}
      </ol>
      <div className="feature-media" id="feature-preview" aria-live="polite" aria-atomic="true">
        <div className={`feature-media-inner feature-media-${feature.id}${feature.images.length > 1 ? " is-stacked" : ""}`} key={feature.id}>
          {feature.artwork === "frames" && <SpeedFrames label={feature.alt} />}
          {feature.images.length > 0 && <div className="feature-artwork" role="img" aria-label={feature.alt}>
            {feature.images.map((source, index) => <img key={source} src={asset(source)} alt="" loading="lazy" style={{ "--layer": index, "--layer-position": `${feature.images.length > 1 ? index / (feature.images.length - 1) * 100 : 0}%`, zIndex: feature.images.length - index }} />)}
          </div>}
          <span className="sr-only">{feature.title}</span>
        </div>
      </div>
    </div>
  );
}
