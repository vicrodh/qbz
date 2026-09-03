# Design options (2026-09-03 revamp)

Static, single-file mockups of two alternative directions for the home page.
They are **not** part of the Vite build and are never deployed; open them
straight from disk (they reference `../public/...` relatively):

    xdg-open design-options/option-b-silver.html
    xdg-open design-options/option-c-datasheet.html

- **Option A — "Front panel"** is the one actually built into `src/`
  (branch `website-revamp`): chassis black, cobalt from the logo, an amber
  DAC-style readout under the hero capture, Archivo expanded nameplates.
- **Option B — "Silver faceplate"**: the same vocabulary on a light,
  brushed-aluminium ground with black display windows for the captures.
  Condensed uppercase nameplate headline, centered hero.
- **Option C — "Datasheet"**: utilitarian, single column, tables instead of
  cards, plain white, one link colour. The MusicBee school: everything on one
  screen, zero decoration.

All three keep the download pyramid (Linux on top, macOS and Windows below)
and every platform disclaimer.
