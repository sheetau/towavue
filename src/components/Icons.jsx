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
  close: "m7 7 10 10M7 17 17 7",
  image: "M3 3h18v18H3zM3 16l5-5 4 4 3-3 6 6M15 7h.01",
  video: "m9 6 10 6-10 6z",
  audio: "M9 18V5l11-2v13M9 9l11-2M9 18c0 2-2 3-4 3s-3-1-3-2 1-3 4-3h3m11 0c0 2-2 3-4 3s-3-1-3-2 1-3 4-3h3",
  book: "M12 5v16M12 5C9 3 5 3 2 4v15c3-1 7-1 10 2 3-3 7-3 10-2V4c-3-1-7-1-10 1",
  muted: "m4 10 5-4v12l-5-4H1v-4h3m10-1 7 7m0-7-7 7",
};

export function Icon({ name, className = "" }) {
  return <svg className={`icon ${className}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name] ?? paths.arrow} /></svg>;
}
