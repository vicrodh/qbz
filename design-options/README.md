# Design options (2026-09-03 revamp)

Static, single-file mockups of two alternative directions for the home page.
They are **not** part of the Vite build and are never deployed; open them
straight from disk (they reference `../public/...` relatively):

    xdg-open design-options/option-b-silver.html
    xdg-open design-options/option-c-datasheet.html

- **Option A** is the one actually built into `src/` (branch
  `website-revamp`): one typeface (Archivo), sentence case, centered hero
  with one big capture, alternating bands with one picture each, real product
  photos (Kiosk on a handheld, qbzd in a terminal), cobalt from the logo as
  the only accent. The first cut (mono eyebrows, amber readout, hairline
  card grids) was rejected as AI-looking and rebuilt; B and C still carry
  some of that vocabulary and are kept only as layout references.
- **Option B — "Silver faceplate"**: the same vocabulary on a light,
  brushed-aluminium ground with black display windows for the captures.
  Condensed uppercase nameplate headline, centered hero.
- **Option C — "Datasheet"**: utilitarian, single column, tables instead of
  cards, plain white, one link colour. The MusicBee school: everything on one
  screen, zero decoration.

All three keep the download pyramid (Linux on top, macOS and Windows below)
and every platform disclaimer.
