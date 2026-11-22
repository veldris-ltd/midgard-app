/* The three public tile hosts, for the `public` build only: a demo outside the
 * enclosure. [FR-H-1] [NFR-PRV-1]
 *
 * Only `active.ts` refers to this module, and only when `__HEIMDALL_TILE_PUBLIC__` is
 * true, so neither a `mirror` nor a `none` build holds these host names. Pure. */
import type { TileSources } from './hosts.ts';

export function publicSources(): TileSources {
  return {
    // Mapzen Terrain Tiles on AWS Open Data, terrarium encoding.
    terrain: {
      id: 'terrain',
      origin: 'https://s3.amazonaws.com',
      pathPrefix: '/elevation-tiles-prod/terrarium/',
      order: 'zxy',
      ext: '.png',
      minZoom: 0,
      maxZoom: 15,
    },
    // EOX Sentinel-2 cloudless, the 2016 layer (CC BY 4.0) only, Web Mercator set "g".
    imagery: {
      id: 'imagery',
      origin: 'https://tiles.maps.eox.at',
      pathPrefix: '/wmts/1.0.0/s2cloudless_3857/default/g/',
      order: 'zyx',
      ext: '.jpg',
      minZoom: 0,
      maxZoom: 14,
    },
    // VersaTiles: OpenStreetMap vector tiles in the Shortbread schema.
    vector: {
      id: 'vector',
      origin: 'https://tiles.versatiles.org',
      pathPrefix: '/tiles/osm/',
      order: 'zxy',
      ext: '',
      minZoom: 0,
      maxZoom: 14,
    },
  };
}
