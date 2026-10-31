# Ambient fish background

Visual inspiration: [Dao Ho's portfolio](https://github.com/Dao-Ho/portfolio),
observed at revision `914651c33f254e52bfee9dca5164264c2c33ebfe`.
Its typographic pond motivated the use of small grayscale tiles and glyphs.

`src/site/ambient-fish.mjs` and `src/components/AmbientFish.jsx` are a new
implementation for towavue. No upstream source files or art assets are included.
The renderer uses independently drawn silhouettes, continuous orbital paths,
an offscreen shape mask, and a cached glyph atlas. There are no eyes, barbels,
feeding controls, cursor reactions, analytics, or external runtime dependencies.
