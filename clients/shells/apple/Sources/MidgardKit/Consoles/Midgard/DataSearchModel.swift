import Foundation
@preconcurrency import MidgardCore

/// Data Search (`DataSearchScreen.tsx`): pick an object type, add conditions on its
/// properties (`eq ne lt lte gt gte contains`, `*` for any text), order the matches and
/// page through them; or, with text from the header's search (`?q=`), look for it in
/// every type at once. Each search is `midgard.query` through the core, so it is
/// authorised and audited, and every count covers only what the persona may read.
/// Offline, the last answer to the same search is shown and said to be the last one;
/// nothing is queued, since a search is a read. [DD-1] [NFR-SEC-2] [FR-C-4]
@MainActor
public final class DataSearchModel: ObservableObject {
    public static let page = 50
    /// Matches shown per type when searching every type for text.
    public static let perType = 20
    /// The pseudo-property for "any text property".
    public static let anyText = "*"

    public static let opLabel: [String: String] = [
        "eq": "is", "ne": "is not", "lt": "<", "lte": "≤", "gt": ">", "gte": "≥", "contains": "contains",
    ]

    /// The comparisons that make sense for a property of `type` (nil: any text).
    public static func ops(for type: String?) -> [String] {
        guard let type else { return ["contains"] }
        if type == "text" { return ["contains", "eq", "ne", "lt", "lte", "gt", "gte"] }
        if type == "bool" { return ["eq", "ne"] }
        return ["eq", "ne", "lt", "lte", "gt", "gte"]
    }

    public struct Condition: Equatable {
        public var property: String
        public var op: String
        public var value: String
    }

    /// One search as it was run, so editing the form changes nothing until Search.
    public struct Spec: Equatable {
        public let objectType: String
        public let conditions: [Condition]
        public let orderBy: String
        public let descending: Bool
        public let offset: Int
    }

    public struct TextHit: Equatable {
        public let type: MidgardObjectType
        public let results: [MidgardFoundObject]
        public let total: Int
    }

    @Published public private(set) var types: [MidgardObjectType] = []
    @Published public private(set) var typeName = ""
    @Published public var conditions: [Condition] = []
    @Published public var orderBy = ""
    @Published public var descending = false
    @Published public private(set) var spec: Spec?
    @Published public private(set) var answer: MidgardQueryAnswer?
    @Published public private(set) var auditId: String?
    @Published public private(set) var error: String?
    @Published public private(set) var searching = false
    /// Set when the answer shown is the last one this device got, not a new one.
    @Published public private(set) var offlineNote: String?
    @Published public var selected = 0
    /// The header's text search (`?q=`), and what it found per type.
    @Published public private(set) var text = ""
    @Published public private(set) var textHits: [TextHit]?

    private let app: AppModel
    private var cache: [String: (MidgardQueryAnswer, String?)] = [:]

    public init(app: AppModel) {
        self.app = app
    }

    public var type: MidgardObjectType? { types.first { $0.apiName == typeName } ?? types.first }
    public var properties: [MidgardObjectTypeProperties] { type?.properties ?? [] }

    func typeOf(_ property: String) -> String? { properties.first { $0.name == property }?.typeName }

    public func ops(for condition: Condition) -> [String] {
        Self.ops(for: condition.property == Self.anyText ? nil : typeOf(condition.property))
    }

    public func load() async {
        types = await app.midgardRows("midgard.objectTypes") {
            if case let .midgardObjectType(item) = $0 { return item }
            return nil
        }.rows.map(\.item)
        if typeName.isEmpty || !types.contains(where: { $0.apiName == typeName }) { typeName = types.first?.apiName ?? "" }
    }

    public func pickType(_ apiName: String) {
        typeName = apiName
        conditions = []
        orderBy = ""
    }

    public func addCondition() {
        if let first = properties.first {
            conditions.append(Condition(property: first.name, op: Self.ops(for: first.typeName)[0],
                                        value: first.typeName == "bool" ? "true" : ""))
        } else {
            conditions.append(Condition(property: Self.anyText, op: "contains", value: ""))
        }
    }

    public func removeCondition(_ index: Int) {
        guard conditions.indices.contains(index) else { return }
        conditions.remove(at: index)
    }

    /// Change one condition, keeping its comparison one its property allows.
    public func setCondition(_ index: Int, property: String? = nil, op: String? = nil, value: String? = nil) {
        guard conditions.indices.contains(index) else { return }
        var c = conditions[index]
        if let property { c.property = property }
        if let op { c.op = op }
        if let value { c.value = value }
        let allowed = ops(for: c)
        if !allowed.contains(c.op) { c.op = allowed[0] }
        conditions[index] = c
    }

    public func run(offset: Int = 0) async {
        guard let type else { return }
        let spec = Spec(objectType: type.apiName, conditions: conditions.filter { !$0.value.isEmpty }, orderBy: orderBy,
                        descending: descending, offset: max(0, offset))
        self.spec = spec
        selected = 0
        await query(spec)
    }

    public func previous() async {
        guard let spec, spec.offset > 0 else { return }
        await run(spec, offset: max(0, spec.offset - Self.page))
    }

    public func next() async {
        guard let spec, to < total else { return }
        await run(spec, offset: spec.offset + Self.page)
    }

    private func run(_ s: Spec, offset: Int) async {
        let moved = Spec(objectType: s.objectType, conditions: s.conditions, orderBy: s.orderBy, descending: s.descending,
                         offset: offset)
        spec = moved
        selected = 0
        await query(moved)
    }

    static func whereJSON(_ conditions: [Condition]) -> String {
        let list = conditions.map { ["property": $0.property, "op": $0.op, "value": $0.value] }
        let data = (try? JSONSerialization.data(withJSONObject: list, options: [.sortedKeys])) ?? Data("[]".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    private static func key(_ s: Spec, max: Int) -> String {
        "\(s.objectType)|\(whereJSON(s.conditions))|\(s.orderBy)|\(s.descending)|\(s.offset)|\(max)"
    }

    private enum Asked {
        /// The answer, its audit id, and whether it is the last one this device got.
        case answer(MidgardQueryAnswer, String?, Bool)
        case failure(String)
    }

    /// One `midgard.query`, or offline the last answer to it.
    private func ask(_ s: Spec, max: Int) async -> Asked {
        let key = Self.key(s, max: max)
        if app.isOffline {
            if let (answer, audit) = cache[key] { return .answer(answer, audit, true) }
            return .failure("Offline: this search has not been run on this device yet, so there is no last answer to show.")
        }
        var params: [(String, String)] = [("maxResults", String(max)), ("offset", String(s.offset))]
        if !s.conditions.isEmpty { params.append(("where", Self.whereJSON(s.conditions))) }
        if !s.orderBy.isEmpty { params.append(("orderBy", s.orderBy)) }
        if s.descending { params.append(("descending", "true")) }
        switch await app.midgardRun("midgard.query", "midgard.objectTypes/\(s.objectType)", params) {
        case let .applied(audit, result):
            guard let answer = midgardDecode(MidgardQueryAnswer.self, result) else {
                return .failure("The enclosure's answer did not decode.")
            }
            cache[key] = (answer, audit)
            return .answer(answer, audit, false)
        case let .refused(why), let .conflict(why), let .failed(why):
            return .failure(why)
        case .queued:
            return .failure("Offline: this search has not been run on this device yet, so there is no last answer to show.")
        }
    }

    private func query(_ s: Spec) async {
        searching = true
        error = nil
        answer = nil
        offlineNote = nil
        switch await ask(s, max: Self.page) {
        case let .answer(answer, audit, cached):
            self.answer = answer
            auditId = audit
            offlineNote = cached ? "OFFLINE · SHOWING THE LAST ANSWER THIS DEVICE GOT" : nil
        case let .failure(why):
            error = why
        }
        searching = false
    }

    public var results: [MidgardFoundObject] { answer?.results ?? [] }
    public var total: Int { answer?.total ?? results.count }
    public var from: Int { spec.map { $0.offset + 1 } ?? 0 }
    public var to: Int { spec.map { $0.offset + results.count } ?? 0 }
    public var picked: MidgardFoundObject? { results.isEmpty ? nil : results[min(selected, results.count - 1)] }
    public var columns: [String] {
        types.first { $0.apiName == spec?.objectType }?.properties?.map(\.name) ?? []
    }

    /// Search every type for `text` (the header's search box).
    public func search(text: String) async {
        let text = text.trimmingCharacters(in: .whitespaces)
        self.text = text
        textHits = nil
        guard !text.isEmpty else { return }
        if types.isEmpty { await load() }
        searching = true
        var hits: [TextHit] = []
        let any = [Condition(property: Self.anyText, op: "contains", value: text)]
        for t in types {
            let s = Spec(objectType: t.apiName, conditions: any, orderBy: "", descending: false, offset: 0)
            if case let .answer(answer, _, _) = await ask(s, max: Self.perType) {
                hits.append(TextHit(type: t, results: answer.results, total: answer.total))
            }
        }
        textHits = hits
        searching = false
    }

    /// Leave the text search for one type's matches ("See all" / "Refine").
    public func refine(_ hit: TextHit) async {
        let any = [Condition(property: Self.anyText, op: "contains", value: text)]
        text = ""
        textHits = nil
        typeName = hit.type.apiName
        conditions = any
        orderBy = ""
        descending = false
        await run()
    }

    public func clearText() {
        text = ""
        textHits = nil
    }

    /// The conditions as the result badges and the terminal say them.
    public static func describe(_ c: Condition) -> String {
        "\(c.property == anyText ? "any text" : c.property) \(opLabel[c.op] ?? c.op) \(c.value)"
    }
}
