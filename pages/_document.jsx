import { Head, Html, Main, NextScript } from "next/document";
import { asset } from "../src/site/config.mjs";

export default function Document(props) {
  const locale = props.__NEXT_DATA__?.props?.pageProps?.locale ?? "en";
  return <Html lang={locale}>
    <Head>
      <meta name="theme-color" content="#000000" />
      <meta name="color-scheme" content="dark" />
      <meta name="darkreader-lock" />
      <link rel="preload" href={asset("fonts/atelier-femme-serif.ttf")} as="font" type="font/ttf" crossOrigin="anonymous" />
      <link rel="preload" href={asset("fonts/helvetica-now-text.ttf")} as="font" type="font/ttf" crossOrigin="anonymous" />
      <style dangerouslySetInnerHTML={{ __html: `
        @font-face { font-family: "Atelier Femme Serif"; src: url("${asset("fonts/atelier-femme-serif.ttf")}") format("truetype"); font-weight: 400; font-style: normal; font-display: swap; }
        @font-face { font-family: "Helvetica Now Text"; src: url("${asset("fonts/helvetica-now-text.ttf")}") format("truetype"); font-weight: 400; font-style: normal; font-display: swap; }
      ` }} />
      {locale === "ja" && <>
        <link rel="preload" href={asset("fonts/towavue-jp-narrow-full.woff2")} as="font" type="font/woff2" crossOrigin="anonymous" />
        <style dangerouslySetInnerHTML={{ __html: `
          @font-face { font-family: "Towavue JP Narrow"; src: url("${asset("fonts/towavue-jp-narrow-full.woff2")}") format("woff2"); font-weight: 400; font-style: normal; font-display: swap; }
        ` }} />
      </>}
    </Head>
    <body><Main /><NextScript /></body>
  </Html>;
}
