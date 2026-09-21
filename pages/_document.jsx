import { Head, Html, Main, NextScript } from "next/document";

export default function Document(props) {
  const locale = props.__NEXT_DATA__?.props?.pageProps?.locale ?? "en";
  return (
    <Html lang={locale}>
      <Head>
        <meta name="theme-color" content="#000000" />
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="anonymous" />
        <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Serif:ital,wght@0,400;1,400&display=swap" />
      </Head>
      <body><Main /><NextScript /></body>
    </Html>
  );
}
