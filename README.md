# towavue website

Landing page for [towavue](https://github.com/sheetau/towavue), based on the finished Frame 3 in `concepts/lp_concept.fig` and `concepts/plan.txt`.

Website: **https://sheetau.github.io/towavue/**

## Local development

Requires Node.js 22 or later. Generated assets are committed; a normal build needs no Python or font installation.

```powershell
npm ci
npm run dev
```

Open `http://localhost:3000/towavue/`; Japanese is at `/towavue/ja/`.

```powershell
npm run check
npm run build
npm run preview
```

Static output is in `out/`. Development uses `.next-dev/`, production uses `.next/`. Set `PORT` to change the preview port. For a phone on the same LAN, use `npm run preview:lan` and open the printed URL. Rebuild and reload after changes to the static preview.

## Design and interactions

The October 13, 2026 Frame 3 export defines a 1145px content width, black background, Atelier Femme Serif headings/introductions, Helvetica Now Text body text, and four feature sections. Shared layout, spacing, color, and typography tokens remain in `styles/globals.css`. The supplied font files are served locally. Japanese uses Noto Serif JP as its serif fallback.

- Lenis smooth wheel scrolling uses interpolation (`lerp: 0.1`), matching the Pixel Reveal reference. Gallery and scroll progress follow the same native page position. Touch scrolling stays native and reduced-motion preferences disable smoothing.
- Transparent sticky header with difference blending; its bottom line reflects document scroll progress.
- The header logo uses 18 solid rectangular rods arranged on six cube faces, built directly from the three-line SVG artwork. Each rod extends about 1 CSS pixel inward, preserving the front outline, with subtle side shading and depth fading. A depth prepass prevents overlapping rod surfaces from brightening one another. No SVG texture or face-edge fade is needed. Hover or keyboard focus starts a 1.3-second 90-degree turn with mild perspective, alternating left and up; a slow launch accelerates into a damped spring with about 8.5 degrees of overshoot and a 0.8-degree rebound; another starts after 3 seconds at rest, preserving the 4.3-second start-to-start cadence. Opposite artwork aligns, and each incoming face matches the static logo after a quarter-turn. Depth fog remains fixed during rotation. The original SVG appears between turns, with a 320ms crossfade at either end of the rotation reaching 80% opacity on both layers at the midpoint. Rendering stops while idle, offscreen, or in a hidden tab; reduced motion and unavailable WebGL retain the SVG. Geometry, timing, and rendering live in `src/site/logo-cube.mjs`, with lifecycle and triggers in `src/components/HeaderLogo.jsx`. No additional runtime dependency is used.
- One gallery tab containing 20 licensed archive images. Each image has the same diagonal and retains its aspect ratio. The gallery follows page scrolling through every row while the final row remains visible; direct wheel scrolling is disabled. Reduced-motion preferences stop this automatic effect. Caption controls retain decorative hover states. The old media-tab, playback, and video-scroll code has been removed.
- Benchmark comparisons in IrfanView / MPC-HC / VLC order, with two bars each on a shared 0-400 completion-time scale. Bars animate once when entering view; reduced motion and no-JavaScript rendering show complete bars. The values and explanation match Frame 3.
- Three workspace screenshots copied directly from `workspace1-3.png`.
- Interactive waveform with screen blending. Left click/drag seeks, dotted edges resize the selection, and right drag within the selection moves it. Touch can move the selected region. Selection width cannot fall below 6% of the timeline. All four controls also support arrows, Shift+arrow, Home, and End.
- Fifteen-image filmstrip with an animated set of corner brackets and moving file position/name. Hover, tap, focus, and arrow-key navigation update the active image. The mobile strip scrolls horizontally inside its own viewport.
- Timeline selection and filmstrip brackets have one-time, in-view entrances using the benchmark easing. Interacting with the controls cancels the entrance immediately; reduced motion skips it.
- Main download buttons use a white/gray fluid shader on hover or keyboard focus. The header keeps its plain hover. GPU drawing stops while idle or offscreen, and reduced motion holds a still frame.

The page declares its native dark palette and includes Dark Reader's documented lock to prevent double transformation of its black background, white bars, and difference-blended header.

## Organization

| Location | Responsibility |
| --- | --- |
| `pages/` | Static routes and document/font metadata |
| `src/components/LandingPage.jsx` | Page composition and SEO metadata |
| `src/components/HeroDemo.jsx` | Single gallery and scroll response |
| `src/components/Benchmark.jsx` | Comparison data and reveal animation |
| `src/components/TimelineDemo.jsx` | Pointer and keyboard timeline controls |
| `src/components/FilmstripDemo.jsx` | Thumbnail selection, brackets, and metadata |
| `src/site/locales.js`, `src/site/ja.json` | English and Japanese copy |
| `src/site/gallery.json` | Generated image metadata |
| `src/site/timeline.mjs` | Shared selection bounds and diagonal sizing |
| `src/site/config.mjs`, `src/site/download.mjs` | URLs and latest Windows installer lookup |
| `styles/globals.css` | Shared design tokens and responsive layout |
| `public/media/` | Supplied artwork, generated thumbnails, fonts, and existing assets |

The original `concepts/` and `images/` directories remain local and ignored. The app checkout stays separate.

## Downloads, localization, and metadata

All three download links resolve the latest public Windows x64 setup at click time, with a shared status message and Releases fallback on errors. Without JavaScript they remain ordinary release links. Installer selection and failure handling are covered by tests. Download labels do not change during loading.

The existing language menu retains keyboard navigation, outside-click dismissal, and real locale links. Both routes have canonical and reciprocal hreflang URLs, Open Graph/Twitter images, and SoftwareApplication structured data. The supplied product preview and favicon remain unchanged. Add locales through `src/site/locales.js`, a corresponding page route, and `public/sitemap.xml`.

Google Analytics retains measurement ID `G-C4WW8W4TVG` and the `download_click` event, but initializes only on `sheetau.github.io` so local preview visits do not reach the production property.

## Media preparation

The gallery contains 20 licensed archive images. The filmstrip uses the first 15 images in the gallery order, defined by `filmstripIds` in the gallery manifest. Source URLs, creator credits, image licences, and conversion notes are recorded in `src/site/gallery.json` and `public/media/gallery/credits.json`, and displayed at `/image-credits/`. Image licences are separate from the website code licence.

To recreate the proportional WebP thumbnails (Python, Pillow, requests):

```powershell
python scripts/prepare-archive-media.py
```

Original downloads are cached in ignored `output/archive-sources/`. Images are not cropped or recoloured. `scripts/prepare-media.py` still copies the supplied workspace artwork and Latin fonts, while preserving the archive catalogue.

Japanese serif glyphs use **Towavue JP Narrow**, a renamed Noto Serif JP Regular derivative whose outlines and horizontal advances are scaled to 70%. It preserves the source font's full character coverage, alternate glyphs, and ideographic variation sequences. Atelier Femme Serif remains first in the font stack, so English text keeps its original width. The font is self-hosted with its SIL OFL licence. A preloaded page subset is about 180KB instead of 2.98MB; disjoint Unicode ranges load the full narrow font only for characters outside that subset. Japanese serif text stays hidden until its actual narrow faces load, without a timed fallback reveal. M PLUS 1 Code Regular is also self-hosted (about 43KB for the page subset), so iOS no longer depends on an installed font. Its full-coverage fallback and OFL licence are included. Editing copy remains safe without regeneration; rerun `python scripts/prepare-japanese-webfonts.py` after copy changes to keep the first load small. This script uses fonttools[woff] and requests, generates `src/site/japanese-fonts.json`, and requires no Python during a normal site build. To regenerate the full font (Python, requests, fonttools, brotli):

```powershell
python scripts/prepare-japanese-serif.py
```

## Publishing

The app remains on `main` in its own folder. This folder is a separate Git repository whose branch is `gh-pages`, with the same remote, `https://github.com/sheetau/towavue.git`. Do not merge the website branch into `main`.

GitHub repository **Settings → Pages → Build and deployment → Source** must be **GitHub Actions**. The included workflow runs only for pushes to `gh-pages` and reports a `github-pages` deployment on the app repository.

After reviewing a site change:

```powershell
npm run check
npm run build
git add pages src styles public scripts tests package.json package-lock.json next.config.mjs README.md .github
git commit -m "Update towavue website"
git push origin gh-pages
```

If setting up the branch manually for the first time, initialize this folder with `git init -b gh-pages`, add the remote with `git remote add origin https://github.com/sheetau/towavue.git`, commit the website files, and use `git push -u origin gh-pages`. No operation in the app folder is needed. A push triggers the deploy workflow; track it in the repository's Actions tab.

For a different hosting subpath set `NEXT_PUBLIC_BASE_PATH` before both build and preview (default `/towavue`; empty for the domain root). Update the canonical domain in `src/site/config.mjs` if hosting outside GitHub Pages.

## Verification

- `npm run check`: installer resolution/error cases, timeline minimum width and boundary preservation, image aspect ratio/diagonal sizing.
- `npm run build`: English and Japanese static exports.
- Browser review: desktop and mobile layouts, asset loading, bar reveal, gallery scroll response, timeline drag/keyboard boundaries, filmstrip movement, locale navigation, and horizontal overflow.

Local design extraction and visual-review artifacts are in ignored `output/frame3/`.

### Fluid background credit

The download-button fluid shading is adapted from [Fluid Orb by Rare UI](https://rareui.com/components/fluidorb), copyright (c) 2026 Swami Malode. It uses the MIT + Commons Clause + Attribution terms in [third-party/rare-ui-LICENSE.txt](third-party/rare-ui-LICENSE.txt), separately from the website code licence. The adaptation fills a rectangular button, reads the existing grayscale tokens, and renders only while active.
