export function Logo({ className = "" }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 27.68 27.68" fill="none" stroke="currentColor" strokeWidth="2" strokeMiterlimit="10" aria-hidden="true">
      <line x1="2.17" y1="2.17" x2="9.82" y2="9.82" />
      <line x1="2.17" y1="25.5" x2="10.21" y2="17.47" />
      <line x1="17.47" y1="10.21" x2="25.5" y2="2.17" />
      <path d="M9.82,1h-4.82c-2.21,0-4,1.79-4,4v4.82" />
      <path d="M26.68,10.21v-5.21c0-2.21-1.79-4-4-4h-5.21" />
      <path d="M17.47,26.68h5.21c2.21,0,4-1.79,4-4v-5.21" />
      <path d="M1,17.86v4.82c0,2.21,1.79,4,4,4h4.82" />
    </svg>
  );
}

const paths = {
  arrow: "M7 17 17 7M7 7h10v10",
  download: "M12 3v12m-5-5 5 5 5-5M5 16v5h14v-5",
  chevron: "m8 14 4-4 4 4",
  // Lucide, pinned to the same upstream revision as the app. See notices/lucide.md.
  play: "M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z",
  book: "M12 5v16 M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z",
  muted: "M11 4.702a.7.7 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.7.7 0 0 0 11 19.298z M16.5 14.5l5-5 M16.5 9.5l5 5",
  repeat: "m17 2 4 4-4 4 M3 11v-1a4 4 0 0 1 4-4h14 m-14 16-4-4 4-4 M21 13v1a4 4 0 0 1-4 4H3",
  shuffle: "m18 14 4 4-4 4 m0-20 4 4-4 4 M2 18h1.973a4 4 0 0 0 3.3-1.7l5.454-8.6a4 4 0 0 1 3.3-1.7H22 M2 6h1.972a4 4 0 0 1 3.6 2.2 M22 18h-6.041a4 4 0 0 1-3.3-1.8l-.359-.45",
};

export function Icon({ name, className = "" }) {
  return <svg className={`icon ${className}`} viewBox="0 0 24 24" fill={name === "play" || name === "pause" ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {name === "pause" ? <><rect x="14" y="3" width="5" height="18" rx="1" /><rect x="5" y="3" width="5" height="18" rx="1" /></> : <path d={paths[name === "repeat-one" ? "repeat" : name] ?? paths.arrow} />}
    {name === "repeat-one" && <path d="M11 10h1v4" />}
  </svg>;
}

// The close and caption artwork matches the monapad LP exactly.
export function TabClose() {
  return <span className="editor-tab-close" aria-hidden="true"><svg viewBox="60 0 7.5 7.5" fill="none"><path d="M66.72.5 60.22 7M66.72 7 60.22.5" stroke="currentColor" strokeWidth="1" strokeLinecap="round" /></svg></span>;
}

export function WindowControls() {
  return <div className="editor-window-controls" aria-hidden="true"><svg viewBox="0 0 67.22 7.5" fill="none"><path d="M66.72.5 60.22 7M66.72 7 60.22.5M.5 3.75H7" stroke="currentColor" strokeWidth="1" strokeLinecap="round" /><rect x="30.36" y=".5" width="6.5" height="6.5" rx=".76" stroke="currentColor" strokeWidth="1" /></svg></div>;
}
