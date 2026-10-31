import Head from "next/head";
import { asset, localePath } from "../src/site/config.mjs";
import media from "../src/site/gallery.json";

export default function ImageCredits() {
  return <>
    <Head><title>Image credits | towavue</title><meta name="robots" content="noindex" /></Head>
    <main className="container image-credits-page">
      <a href={localePath("en")}>← towavue</a>
      <h1>Image credits</h1>
      <p>Photographs, scientific illustrations, and satellite observations used in the gallery and filmstrip. Each image retains its own licence, independently of the website code.</p>
      <p lang="ja">ギャラリーとフィルムストリップで使用している写真・科学図版・衛星画像の出典です。画像には、それぞれ下記のライセンスが適用されます。</p>
      <div className="image-credits-grid">{media.photos.map((photo, index) => <article key={photo.src}>
        <a href={asset(photo.src)}><img src={asset(photo.src)} width={photo.width} height={photo.height} alt={photo.title} loading="lazy" /></a>
        <h2>{String(index + 1).padStart(2, "0")} / {photo.title}</h2>
        <p>{photo.creator}{photo.date && `, ${photo.date}`}</p>
        <p><a href={photo.sourceUrl}>{photo.source}</a></p>
        <p><a href={photo.licenseUrl}>{photo.license}</a></p>
        <p>{photo.changes}</p>
      </article>)}</div>
      <p>Japanese sans-serif: M PLUS 1 Code (M+ FONTS). <a href={asset("fonts/m-plus-1-code-OFL.txt")}>SIL Open Font License 1.1</a>.</p>
      <p>Japanese serif: Towavue JP Narrow, derived from Noto Serif JP (Google). <a href={asset("fonts/towavue-jp-narrow-OFL.txt")}>SIL Open Font License 1.1</a>.</p>
    </main>
  </>;
}
