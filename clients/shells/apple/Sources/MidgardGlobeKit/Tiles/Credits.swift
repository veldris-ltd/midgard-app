import Foundation

/// One data source a view draws from, with its licence and the attribution the licence
/// asks for, word for word (the web's `Credit`). [FR-H-1]
public struct Credit: Equatable, Sendable, Identifiable {
    public let id: String
    /// What the source is, as the user sees it.
    public let name: String
    /// The short attribution for the always-visible credits strip.
    public let short: String
    public let licence: String
    public let attribution: String

    /// The bundled outlines (`app/dataSources.ts`).
    public static let naturalEarth = Credit(
        id: "natural-earth",
        name: "Natural Earth 5.1.2 · coastlines, land and borders (bundled)",
        short: "Natural Earth",
        licence: "Public domain",
        attribution: "Made with Natural Earth. Free vector and raster map data @ naturalearthdata.com.")

    /// Terrain heights (`tiles/credits.ts`).
    public static let terrain = Credit(
        id: "terrain",
        name: "Mapzen Terrain Tiles (AWS Open Data) · terrain heights",
        short: "Terrain: Mapzen, AWS Open Data",
        licence: "Open data, attribution required (public domain, OGL, CC BY sources)",
        attribution: "Mapzen. ArcticDEM terrain data DEM(s) were created from DigitalGlobe, Inc., imagery and funded under "
            + "National Science Foundation awards 1043681, 1559691, and 1542736; Australia terrain data © Commonwealth of "
            + "Australia (Geoscience Australia) 2017; Austria terrain data © offene Daten Österreichs – Digitales "
            + "Geländemodell (DGM) Österreich; Canada terrain data contains information licensed under the Open Government "
            + "Licence – Canada; Europe terrain data produced using Copernicus data and information funded by the European "
            + "Union - EU-DEM layers; Global ETOPO1 terrain data U.S. National Oceanic and Atmospheric Administration; "
            + "Mexico terrain data source: INEGI, Continental relief, 2016; New Zealand terrain data Copyright 2011 Crown "
            + "copyright (c) Land Information New Zealand and the New Zealand Government (All rights reserved); Norway "
            + "terrain data © Kartverket; United Kingdom terrain data © Environment Agency copyright and/or database right "
            + "2015. All rights reserved; United States 3DEP (formerly NED) and global GMTED2010 and SRTM terrain data "
            + "courtesy of the U.S. Geological Survey.")

    /// Satellite imagery: the 2016 layer only.
    public static let imagery = Credit(
        id: "imagery",
        name: "Sentinel-2 cloudless 2016 by EOX · satellite imagery",
        short: "Imagery: Sentinel-2 cloudless 2016 by EOX (CC BY 4.0)",
        licence: "CC BY 4.0",
        attribution: "EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus "
            + "Sentinel data 2016)")

    /// The geographic map's vector tiles.
    public static let vector = Credit(
        id: "vector",
        name: "OpenStreetMap, as VersaTiles vector tiles · the geographic map",
        short: "© OpenStreetMap contributors · VersaTiles",
        licence: "ODbL 1.0 (data); tiles by VersaTiles",
        attribution: "© OpenStreetMap contributors (openstreetmap.org/copyright). Vector tiles by VersaTiles "
            + "(versatiles.org), Shortbread schema.")
}

/// Pausing map tiles after camera moves the tracks decide, as the web's `tilePause.ts`
/// in its `public` build (which these apps are: the public hosts, for a demo). A tile
/// request names the area in view, so a view framed on the tracks would tell the tile
/// hosts where the tracks are: a move the tracks decide (the first view fitted to them,
/// SECTOR NORTH, flying to a track) pauses tile loading, and only the user's own move
/// that is not centred on a track resumes it (a drag, a zoom, a tilt or turn, or a
/// geographic preset). [FR-H-1] [NFR-PRV-1]
public enum TilePause {
    public enum Move: Sendable { case track, user }

    public static let notice = "MAP TILES PAUSED · PAN AWAY OR CHOOSE A REGION"
    public static let offlineNotice = "MAP TILES NEED A CONNECTION"

    public static func paused(after move: Move) -> Bool { move == .track }

    /// Flying to a region: a track move when it is framed on the tracks, else the user's own.
    public static func move(for region: Region) -> Move { region.fittedToTracks ? .track : .user }
}
