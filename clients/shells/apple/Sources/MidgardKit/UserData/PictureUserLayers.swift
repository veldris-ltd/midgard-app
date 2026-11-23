import Foundation
@preconcurrency import MidgardCore

extension HeimdallPictureModel {
    /// The user's layers that are switched on.
    public var visibleUserLayers: [UserMapLayer] { userLayers.filter { userLayerOn[$0.id] != false } }

    public func setUserLayer(_ id: String, _ on: Bool) {
        userLayerOn[id] = on
    }

    /// While the picture is open: poll the user's feeds, and redraw their layers as
    /// answers arrive. Ends (and stops polling) when the picture closes. [NFR-PRV-1]
    public func followUserData() async {
        let data = app.userData
        data.startPolling()
        defer { data.stopPolling() }
        for await _ in data.objectWillChange.values {
            if Task.isCancelled { break }
            // objectWillChange comes before the change: read it after.
            await Task.yield()
            await refreshUserLayers()
        }
    }

    /// The observation layers, and one layer per imported object type whose objects
    /// carry a position (`lat`/`lon` or `latitude`/`longitude`), from the objects this
    /// persona may read.
    func refreshUserLayers() async {
        var layers = app.userData.observationLayers
        let imported = app.userData.importedTypes
        if !imported.isEmpty, app.userData.live == nil {
            let rows = await app.rows("midgard.objects")
            var byType: [String: [UserMapPoint]] = [:]
            var counts: [String: Int] = [:]
            for row in rows {
                guard case let .midgardObjectInstance(o)? = row.item, imported.contains(o.typeId) else { continue }
                counts[o.typeId, default: 0] += 1
                let values = Dictionary(o.propertyValues.map { ($0.key, $0.value) }, uniquingKeysWith: { a, _ in a })
                if let p = Self.position(values) {
                    let label = values["name"] ?? o.id
                    byType[o.typeId, default: []].append(UserMapPoint(id: o.id, label: label, kind: o.typeId,
                                                                      lat: p.lat, lon: p.lon))
                }
            }
            for type in imported.sorted() where !(byType[type] ?? []).isEmpty {
                layers.append(UserMapLayer(id: "objects:\(type)", name: type, origin: .objects,
                                           points: byType[type] ?? [], total: counts[type] ?? 0))
            }
        }
        if layers != userLayers { userLayers = layers }
    }

    /// A position from float-like properties, in range.
    static func position(_ values: [String: String]) -> (lat: Double, lon: Double)? {
        func value(_ names: [String]) -> Double? {
            for (k, v) in values where names.contains(k.lowercased()) {
                if let x = Double(v), x.isFinite { return x }
            }
            return nil
        }
        guard let lat = value(["lat", "latitude"]), let lon = value(["lon", "lng", "longitude"]),
              (-90...90).contains(lat), (-180...180).contains(lon) else { return nil }
        return (lat, lon)
    }

    /// Where a position falls on the 2D reference layout, in its percent coordinates.
    /// That layout is a schematic (SECTOR NORTH), not a map: the position is placed by
    /// the least-squares fit of the five sample tracks' two layouts (their 2D x/y and
    /// their lat/lon, `SamplePositions`), within about five percent of each. Outside
    /// 0–100 it is off this view (MAP and 3D place it exactly).
    public static func referencePoint(lat: Double, lon: Double) -> (x: Double, y: Double)? {
        let x = 8.2421 * lon - 3.2364 * lat + 245.42
        let y = 0.8949 * lon - 22.690 * lat + 1338.89
        guard (0...100).contains(x), (0...100).contains(y) else { return nil }
        return (x, y)
    }
}
