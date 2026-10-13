import { useEffect, useRef } from "react";
import { repository } from "../site/config.mjs";

const comparisons = [
  { app: "IrfanView", index: 208.4, faster: 108 },
  { app: "MPC-HC", index: 329.1, faster: 229 },
  { app: "VLC", index: 340.4, faster: 240 },
];

export function Benchmark({ content, locale }) {
  const chart = useRef(null);
  useEffect(() => {
    const element = chart.current;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (motion.matches || !("IntersectionObserver" in window)) return;
    const panels = [...element.querySelectorAll(".benchmark-panel")];
    panels.forEach((panel) => { panel.dataset.animate = "true"; });
    // Observe each pair of bars, including on the stacked mobile layout.
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting && entry.intersectionRatio >= 0.85) {
            entry.target.closest(".benchmark-panel").dataset.visible = "true";
            observer.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.85, rootMargin: "0px 0px -64px 0px" },
    );
    panels.forEach((panel) => observer.observe(panel.querySelector("dl")));
    const reveal = () => {
      panels.forEach((panel) => { panel.dataset.visible = "true"; });
      observer.disconnect();
    };
    motion.addEventListener("change", reveal);
    return () => {
      observer.disconnect();
      motion.removeEventListener("change", reveal);
    };
  }, []);

  return (
    <>
      <div className="benchmark-grid" ref={chart}>
        {comparisons.map(({ app, index, faster }, panel) => (
          <article className="benchmark-panel" key={app} style={{ "--panel-delay": `${panel * 100}ms` }}>
            <h3>{locale === "ja" ? `${app}より${faster}%高速` : `${faster}% faster than ${app}`}</h3>
            <dl>
              {[
                { label: "towavue", score: 100 },
                { label: app, score: index },
              ].map(({ label, score }) => (
                <div className={`benchmark-row${score === 100 ? " is-towavue" : ""}`} key={label}>
                  <div className="benchmark-label">
                    <dt>{label}</dt>
                    <dd>{score}</dd>
                  </div>
                  <div className="benchmark-track" aria-hidden="true">
                    <span className="benchmark-bar" style={{ width: `${score / 4}%` }} />
                  </div>
                </div>
              ))}
            </dl>
          </article>
        ))}
      </div>
      <div className="benchmark-method">
        <p>{content.scale}</p>
        <p>{content.method}</p>
        <a
          className="text-link benchmark-link"
          href={`${repository}/blob/main/docs/benchmarks/README.md`}
          target="_blank"
          rel="noreferrer"
        >
          {content.link} ↗
        </a>
      </div>
    </>
  );
}
