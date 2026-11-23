/* The data sources a view draws from, each with its licence and attribution, for the
 * CREDITS control. This file names only what ships inside the console; the map tile
 * sources' credits live with the tile code (src/tiles/credits.ts). [FR-H-1] */

export interface Credit {
  id: string;
  /** What the source is, as the user sees it. */
  name: string;
  licence: string;
  /** The attribution the licence asks for, word for word where it gives one. */
  attribution: string;
}

export const NATURAL_EARTH: Credit = {
  id: 'natural-earth',
  name: 'Natural Earth 5.1.2 · coastlines, land and borders (bundled)',
  licence: 'Public domain',
  attribution: 'Made with Natural Earth. Free vector and raster map data @ naturalearthdata.com.',
};
