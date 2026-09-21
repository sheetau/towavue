import { useState } from "react";
import { asset } from "../site/config.mjs";

export function FeatureShowcase({ content }) {
  const [active, setActive] = useState(0);
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
        <div className="feature-media-inner" key={content.features[active].id}>
          <img src={asset(content.features[active].image)} alt={content.features[active].alt} width="1200" height="760" loading="lazy" />
          <span className="sr-only">{content.features[active].title}</span>
        </div>
        <div className="feature-media-caption" aria-hidden="true"><span>{String(active + 1).padStart(2, "0")} / 05</span><span>{content.features[active].title}</span></div>
      </div>
    </div>
  );
}
