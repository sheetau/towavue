# towavue website

The landing page for [towavue](https://github.com/sheetau/towavue), a Windows media viewer.

Website: **https://sheetau.github.io/towavue/**

## Local development

Requires Node.js 22 or later.

```powershell
npm ci
npm run dev
```

Open `http://localhost:3000/towavue/`. Japanese is available at `/towavue/ja/`.
To preview the exact exported production site, stop the development server first:

```powershell
npm run check
npm run build
npm run preview
```

The preview server supports byte-range video requests. Build output is in `out/`.

## Organization

This follows the monapad LP's Next.js Pages Router, static export, separate website branch, and directory organization:

| Location | Responsibility |
| --- | --- |
| `pages/` | English and Japanese static routes, document metadata, Google Fonts |
| `src/components/` | Page composition and small interactive components |
| `src/site/locales.js` | All page copy, feature definitions, and translated accessibility labels |
| `src/site/config.mjs` | Repository, canonical site URL, base path, and asset paths |
| `src/site/download.mjs` | Latest-release lookup and Windows x64 installer selection |
| `src/site/media.js` | Demo filenames, fictional audio tracks, and clock formatting |
| `src/site/audio-playback.mjs` | Silent audio clock, queue progression, repeat, and shuffle |
| `styles/globals.css` | Shared tokens, page layout, app demo, and responsive rules |
| `public/media/` | Optimized copies of supplied media and the favicon |
| `.github/workflows/deploy.yml` | Build `gh-pages` and deploy `out/` through GitHub Actions |

The original `concepts/` and `images/` directories stay local and ignored. The app checkout is entirely separate. There is no shared worktree, symlink, or app build dependency.

The monapad names `--color-theme-*`, `--font-*`, `--max-width-container`, `.container`, `.hero-text`, `.hero-buttons`, `.app-download`, `.feature-block`, `.feature-showcase`, `.feature-media`, `.prefooter`, `.site-shell`, and `.language-switcher` are reused. Components consume explicit locale objects rather than replacing strings inside serialized HTML; content and interactions stay separate without duplicating markup.

## Design and content

`concepts/lp_concept.fig` was unpacked and its embedded Kiwi schema and Zstandard node data decoded. The frame hierarchy, text styles, fills, and strokes were inspected, alongside the PNG. The source defines a black canvas, white headings, gray body text, IBM Plex Serif headings (italic hero), and IBM Plex Mono body copy. The layout retains the narrow header, left-aligned hero, app demo, five selectable feature rows with a shared preview, three closing cards, and footer. Spacing is normalized through shared tokens.

The body copy is grounded in the app's README, STATUS, ARCHITECTURE, and playlist implementation. The two closing artwork slots without supplied images are deliberately blank on desktop, and collapse on mobile. Add an `image` and `alt` to the corresponding closing item in each locale to fill a slot.

The preview supports image, video, and audio tabs, seven-image navigation, video playback and seeking, and fictional audio tracks with a silent playback clock. The video can be played/paused from either its status button or the media area, and has no audio stream. Audio supports repeat off/all/one and a shuffled queue, without loading an audio source. Switching tabs pauses playback and preserves positions. Window/close controls and the reading icon remain decorative. The supplied outline logo is used for the header and demo; only the browser tab uses the favicon.

The header stays at the top on an opaque black background, with the same inset vertical rules as the content. Screenshot images have no added borders. Demo tabs are rounded, their close/caption SVGs match monapad, and the idle seek bar is a one-pixel track without a visible thumb. Status metadata uses the supplied originals for images, the web video for video, and fictional audio metadata. Transport/reading artwork comes from Lucide; see [attribution and license](notices/lucide.md).

Both download buttons fetch the latest public release at click time and select `towavue-*-windows-x64-setup.exe`. They initiate a browser download without replacing the LP. API errors, timeouts, and missing installers display a retryable inline message and a Releases fallback. The ordinary link remains useful without JavaScript.

## Localization

English is the default. The footer switches between explicit English and Japanese URLs without automatic redirects. To add a language:

1. Add its label to `languages` and its copy to `locales` in `src/site/locales.js`.
2. Add `pages/<locale>/index.jsx` following `pages/ja/index.jsx`.
3. Rebuild and verify the new route and layout.

Google Fonts loads IBM Plex Serif and IBM Plex Mono for English, and Noto Serif JP and M PLUS 1 Code for Japanese. The monapad-style language popup opens above the footer button, marks the current language, and supports arrows, Home/End, Escape, outside-click dismissal, and focus restoration. Metadata includes document language, canonical and alternate URLs, Open Graph, and SoftwareApplication structured data.

## Media

Web-sized derivatives are committed, so a regular site build needs no media tooling. To regenerate from the original local files, install Python/Pillow and FFmpeg, then run:

```powershell
python scripts/prepare-media.py
```

The image samples are the owner's supplied Unsplash files by Ajoy Das, Leman, Magnus Thompson, Michael Navarro, miom, Siddharth Sarma, and Vinh Thang; original photographer identifiers remain in the preparation script. The video derives from supplied `12934697_3840_2160_30fps.mp4`. Source files are preserved. There is no generated replacement artwork. The MP4 is resized to 1280×720, stripped of audio, encoded with frequent keyframes for seeking, and uses fast-start metadata.

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

- `npm run check`: installer selection and failures; paused/ended audio, track transitions, repeat one/all, and shuffle progression.
- `npm run build`: static production output for English and Japanese.
- Browser review: all seven images, keyboard and pointer tab controls, video playback/area-click/seek, silent audio playback/repeat/shuffle, feature previews, language-menu focus and dismissal, download success/failure, responsive overflow, Japanese font loading, inset rails, fixed header, and image borders.
- Download browser checks use the live GitHub release response and intercept the installer payload with a small fixture; they do not run or install the EXE.
- `npm audit`: dependency audit. PostCSS is explicitly overridden to the patched version while retaining the reference site's Next.js 15 structure.

Local visual-review artifacts are kept in ignored `output/playwright/`.
