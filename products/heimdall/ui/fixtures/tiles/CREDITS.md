# Recorded tiles for the tests

Three small map tiles, fetched once on 22 November 2026 and committed so the tests
never touch the network. They are used as they were served, unmodified. Each is
redistributed here under its source's licence, with the credit that licence asks
for.

| File | Source and tile | Licence | SHA-256 |
|---|---|---|---|
| `terrarium-6-31-19.png` | Mapzen Terrain Tiles on AWS Open Data, terrarium encoding, tile 6/31/19 (the Scottish Highlands) | Open data with attribution (see below) | `dd83aaef7c65edb4a7cf5f99f19af3bff4ef2e9d4d3041a7e0e179e4cfcc44a8` |
| `s2cloudless-2016-6-31-19.jpg` | EOX Sentinel-2 cloudless 2016 (`s2cloudless_3857`), tile 6/31/19 | CC BY 4.0 | `4d3d85f748ef72e6223e878220f8cb834c45749835d99354791518eea49768dc` |
| `versatiles-osm-8-125-78.mvt` | VersaTiles OSM vector tiles (Shortbread 1.1), tile 8/125/78 (central Scotland) | ODbL 1.0 (OpenStreetMap data) | `a0678e4749e36838c5b5d9b303d9d95d22ab3cd689606dfd840f3296d809cf9f` |

## Credits

**Terrain.** Mapzen Terrain Tiles, from the Linux Foundation's Mapzen project, on the
AWS Open Data registry. Their required attribution, for the sources in this tile:
Europe terrain data produced using Copernicus data and information funded by the
European Union - EU-DEM layers; Global ETOPO1 terrain data U.S. National Oceanic and
Atmospheric Administration; United Kingdom terrain data © Environment Agency
copyright and/or database right 2015. All rights reserved; United States 3DEP
(formerly NED) and global GMTED2010 and SRTM terrain data courtesy of the U.S.
Geological Survey. The full list is in the repository's `NOTICE`.

**Imagery.** Sentinel-2 cloudless 2016: EOxCloudless https://cloudless.eox.at by EOX
IT Services GmbH (Contains modified Copernicus Sentinel data 2016), released under the
Creative Commons Attribution 4.0 International License.

**Vector tile.** © OpenStreetMap contributors, available under the Open Database
License (https://www.openstreetmap.org/copyright). Tile produced by VersaTiles
(https://versatiles.org). This file is a produced work of the OpenStreetMap database
and stays under the ODbL.
