/* Minimal types for the vector tile decoder used by the traffic overlay. [FR-H-1] */
declare module '@mapbox/vector-tile' {
  export class VectorTile {
    constructor(pbf: unknown);
    layers: Record<string, VectorTileLayer>;
  }
  export interface VectorTileLayer {
    length: number;
    extent: number;
    feature(i: number): VectorTileFeature;
  }
  export interface VectorTileFeature {
    type: number;
    properties: Record<string, unknown>;
    loadGeometry(): { x: number; y: number }[][];
  }
}
declare module 'pbf' {
  export default class Pbf {
    constructor(buf?: ArrayBuffer | Uint8Array);
  }
}
