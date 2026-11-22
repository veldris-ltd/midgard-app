/* The map tile sources' credits, shown by the CREDITS control while each is in use and
 * recorded in NOTICE. Reached only through the tiles switch. [FR-H-1] */
import type { Credit } from '../app/dataSources.ts';

export const TERRAIN_CREDIT: Credit = {
  id: 'terrain',
  name: 'Mapzen Terrain Tiles (AWS Open Data) · terrain heights',
  licence: 'Open data, attribution required (public domain, OGL, CC BY sources)',
  attribution:
    'Mapzen. ArcticDEM terrain data DEM(s) were created from DigitalGlobe, Inc., imagery and funded under National Science Foundation awards 1043681, 1559691, and 1542736; ' +
    'Australia terrain data © Commonwealth of Australia (Geoscience Australia) 2017; ' +
    'Austria terrain data © offene Daten Österreichs – Digitales Geländemodell (DGM) Österreich; ' +
    'Canada terrain data contains information licensed under the Open Government Licence – Canada; ' +
    'Europe terrain data produced using Copernicus data and information funded by the European Union - EU-DEM layers; ' +
    'Global ETOPO1 terrain data U.S. National Oceanic and Atmospheric Administration; ' +
    'Mexico terrain data source: INEGI, Continental relief, 2016; ' +
    'New Zealand terrain data Copyright 2011 Crown copyright (c) Land Information New Zealand and the New Zealand Government (All rights reserved); ' +
    'Norway terrain data © Kartverket; ' +
    'United Kingdom terrain data © Environment Agency copyright and/or database right 2015. All rights reserved; ' +
    'United States 3DEP (formerly NED) and global GMTED2010 and SRTM terrain data courtesy of the U.S. Geological Survey.',
};

export const IMAGERY_CREDIT: Credit = {
  id: 'imagery',
  name: 'Sentinel-2 cloudless 2016 by EOX · satellite imagery',
  licence: 'CC BY 4.0',
  attribution: 'EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016)',
};

export const VECTOR_CREDIT: Credit = {
  id: 'vector',
  name: 'OpenStreetMap, as VersaTiles vector tiles · the geographic map',
  licence: 'ODbL 1.0 (data); tiles by VersaTiles',
  attribution: '© OpenStreetMap contributors (openstreetmap.org/copyright). Vector tiles by VersaTiles (versatiles.org), Shortbread schema.',
};
