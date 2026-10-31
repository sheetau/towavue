import { Head, Html, Main, NextScript } from "next/document";
import { asset } from "../src/site/config.mjs";
import japaneseFonts from "../src/site/japanese-fonts.json";

function fontFaces(family, font) {
  return `
    @font-face { font-family: "${family}"; src: url("${asset(font.page)}") format("woff2"); font-weight: 400; font-style: normal; font-display: block; unicode-range: ${font.pageRange}; }
    @font-face { font-family: "${family}"; src: url("${asset(font.full)}") format("woff2"); font-weight: 400; font-style: normal; font-display: block; unicode-range: ${font.fullRange}; }
  `;
}

// Start before body paint, independently of React hydration. font-display:block
// alone eventually reveals fallback text on a slow connection, so retain this
// gate until the actual narrow face(s) used by the page have finished loading.
const narrowReadyScript = `
  (() => {
    if (!document.fonts) return;
    const root = document.documentElement;
    root.setAttribute("data-jp-serif", "loading");
    const font = '400 24px "Towavue JP Narrow"';
    const selector = 'h1, h2, .benchmark-panel h3, .hero-text > p, .section-heading > p';
    const reveal = () => {
      const text = Array.from(document.querySelectorAll(selector), el => el.textContent).join('') || '???';
      document.fonts.load(font, text).then(faces => {
        if (faces.length && faces.every(face => face.status === 'loaded')) {
          root.setAttribute("data-jp-serif", "ready");
        }
      }).catch(() => {});
    };
    document.fonts.load(font, '???').catch(() => {});
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', reveal, { once: true });
    } else {
      reveal();
    }
    window.addEventListener('online', reveal);
  })();
`;

export default function Document(props) {
  const locale = props.__NEXT_DATA__?.props?.pageProps?.locale ?? "en";
  return <Html lang={locale}>
    <Head>
      <meta name="theme-color" content="#000000" />
      <meta name="color-scheme" content="dark" />
      <meta name="darkreader-lock" />
      {locale === "ja" && <>
        <link rel="preload" href={asset(japaneseFonts.narrow.page)} as="font" type="font/woff2" crossOrigin="anonymous" fetchPriority="high" />
        <link rel="preload" href={asset(japaneseFonts.sans.page)} as="font" type="font/woff2" crossOrigin="anonymous" />
      </>}
      <link rel="preload" href={asset("fonts/atelier-femme-serif.ttf")} as="font" type="font/ttf" crossOrigin="anonymous" />
      <link rel="preload" href={asset("fonts/helvetica-now-text.ttf")} as="font" type="font/ttf" crossOrigin="anonymous" />
      <style dangerouslySetInnerHTML={{ __html: `
        @font-face { font-family: "Atelier Femme Serif"; src: url("${asset("fonts/atelier-femme-serif.ttf")}") format("truetype"); font-weight: 400; font-style: normal; font-display: swap; }
        @font-face { font-family: "Helvetica Now Text"; src: url("${asset("fonts/helvetica-now-text.ttf")}") format("truetype"); font-weight: 400; font-style: normal; font-display: swap; }
        ${fontFaces("M PLUS 1 Code", japaneseFonts.sans)}
      ` }} />
      {locale === "ja" && <>
        <style dangerouslySetInnerHTML={{ __html: `
          ${fontFaces("Towavue JP Narrow", japaneseFonts.narrow)}
          html[data-jp-serif="loading"] :is(h1, h2, .benchmark-panel h3, .hero-text > p, .section-heading > p) { visibility: hidden; }
        ` }} />
        <script dangerouslySetInnerHTML={{ __html: narrowReadyScript }} />
      </>}
    </Head>
    <body><Main /><NextScript /></body>
  </Html>;
}
