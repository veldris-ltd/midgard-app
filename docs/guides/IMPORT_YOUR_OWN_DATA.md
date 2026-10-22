# Import your own data

This guide shows how to put your own data into a Midgard enclosure using only what is in this repository: two JSON files you write by hand (or generate with any tool), and the `mg-admin` and `http-gateway` binaries built from source.

You do not need Likan or any other proprietary tool. Likan is a separate product for designing ontologies; Midgard does not depend on it, and everything below works without it. See [Related proprietary tools](../../README.md#related-proprietary-tools).

For a larger, real example, the [sample datasets](../../samples/README.md) load the same way: `samples/convert.py` writes their `ontology.json` and import file, and their README gives the commands, which import 418 objects and 28 links from live-recorded GB grid, flood, TfL, weather, earthquake and space weather data.

The worked example is a small electricity distribution network: substations, the feeders they supply, and the transformers on those feeders. Every file it uses is in [`examples/utility/`](examples/utility/), and every command and response shown here was run against those files.

---

## How it fits together

| File | What it does | Format |
|---|---|---|
| `ontology.json` | Declares what kinds of thing exist (object types), how they relate (link types) and what can be done to them (actions). | [`contracts/schema/ontology.v1.schema.json`](../../contracts/schema/ontology.v1.schema.json) |
| your data file (`.jsonl`) | The things themselves: one object or one link per line. | [`contracts/schema/import-record.v1.schema.json`](../../contracts/schema/import-record.v1.schema.json) |
| `policy.json` | Which roles may read, and which may run each action. | see [step 6](#6--allow-the-actions) |

The flow is:

1. `mg-admin init` creates a state directory (keys, an empty ontology, a starter policy).
2. You write `ontology.json` into it.
3. `mg-admin import` checks every line of your data file against that ontology and writes it to the enclosure's encrypted store, all or nothing.
4. `mg-admin user add` enrols someone to read it.
5. `http-gateway` serves it, and you query it from the Midgard console or over HTTP.

Steps 1–4 are offline: the gateway must be stopped while they run.

---

## 1 — Build the tools

```bash
cargo build --release -p http-gateway -p mg-admin
```

The binaries land in `./target/release/`. The README's [Run a real enclosure](../../README.md#run-a-real-enclosure-no-demo-data) section covers the same build, including building through the pinned container.

## 2 — Create a state directory

```bash
./target/release/mg-admin init --state-dir ./grid
```

This creates `./grid` with an empty `ontology.json`, a starter `policy.json` and the enclosure's keys. **Treat `./grid/keys/` as a secret.** `init` refuses a directory that already has anything in it.

## 3 — Describe your data: `ontology.json`

Before you can import anything, the enclosure has to know its shape. Copy the example in, or write your own:

```bash
cp docs/guides/examples/utility/ontology.json ./grid/ontology.json
```

An abridged version of [`examples/utility/ontology.json`](examples/utility/ontology.json):

```json
{
  "version": 1,
  "objectTypes": [
    { "id": "substation", "title": "Substation",
      "properties": [ { "name": "name",         "type": "text", "required": true },
                      { "name": "voltage_kv",   "type": "int",  "required": true },
                      { "name": "status",       "type": "text" },
                      { "name": "commissioned", "type": "timestamp" } ] },
    { "id": "transformer", "title": "Transformer",
      "properties": [ { "name": "serial",        "type": "text",  "required": true },
                      { "name": "rating_kva",    "type": "int",   "required": true },
                      { "name": "load_fraction", "type": "float" },
                      { "name": "in_service",    "type": "bool" } ] }
  ],
  "linkTypes": [
    { "id": "transformer_on", "name": "connected to", "from": "transformer", "to": "feeder",
      "cardinality": "many-to-many" }
  ],
  "actions": [
    { "id": "flag_transformer", "name": "flagTransformer", "title": "Flag for inspection",
      "objectType": "transformer", "humanGated": false,
      "marking": "internal", "purposes": ["operations", "asset-management"],
      "effect": { "kind": "set-property", "property": "status", "value": "inspection-due" } }
  ]
}
```

### Object types

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | Machine name. Letters, digits, `_` and `-` only. Becomes the first half of every object's id. |
| `title` | yes | Display name in the console. |
| `properties` | yes | The fields an object of this type may carry (may be empty). |

Each property has a `name` (same character rules as `id`), a `type`, and optionally `"required": true`.

| `type` | JSON value in your data file | Example |
|---|---|---|
| `text` | a string | `"Elm Road primary"` |
| `int` | an integer | `33` |
| `float` | any number | `0.71`, or `71` |
| `bool` | `true` or `false` | `true` |
| `timestamp` | a non-negative integer, Unix seconds (UTC) | `1767225600` |

There is no date-string, list, nested-object or null type. Store a date as Unix seconds, and leave a property out rather than setting it to `null`.

> **Tip:** give a type a `text` property called `name`. The console's search-around graph labels a node with its `name` when it has one, and with its raw id (`transformer/TX-40117`) when it does not.

### Link types

| Key | Meaning |
|---|---|
| `id` | Machine name, used in your data file. |
| `name` | The phrase shown in the console, e.g. `supplied from`. |
| `from`, `to` | The object type `id`s at each end. Both must be declared. |
| `cardinality` | `one-to-one`, `one-to-many` or `many-to-many`. Midgard records it, but the importer does not count links against it, so choose the one that describes your data. |

Links are walked in both directions by search-around, so you only need to declare each relationship once.

### Actions (optional)

An action lets a user change one text property of an object from the console or the API. Leave `"actions": []` if you only want to read your data.

| Key | Meaning |
|---|---|
| `id` | Machine name. **This (or `title`) is what you call the action by** over the API. |
| `name` | The name a `policy.json` rule must grant (see [step 6](#6--allow-the-actions)). |
| `title` | Display name. |
| `objectType` | The type it acts on. |
| `humanGated` | Defaults to **`true`**. A gated action is refused unless the request carries a `humanGateToken`. Set `false` for routine actions. |
| `marking`, `compartments`, `purposes` | Who may run it: the caller needs clearance for the marking and compartments, and one of the purposes. |
| `effect` | `{"kind":"set-property","property":"status","value":"inspection-due"}` sets a value; `{"kind":"cycle-property","property":"status","values":["energised","isolated"]}` steps through a list. The property must be a `text` property of `objectType`. |

### What refuses the file

The gateway reads `ontology.json` at every start and will not start if it is invalid. Unknown keys, a link or action naming an undeclared type, an effect on an undeclared or non-text property, and duplicate ids, property names or action names all refuse it. Every one of these has a worked failing example under [`contracts/fixtures/ontology/`](../../contracts/fixtures/ontology/).

## 4 — Write your data: JSON Lines

A data file has one JSON record per line. Blank lines are skipped. A record is **either an object or a link**, never both.

From [`examples/utility/network.jsonl`](examples/utility/network.jsonl):

```text
{"object":"substation","key":"SS-ELM","marking":"internal","purposes":["operations","asset-management"],"properties":{"name":"Elm Road primary","voltage_kv":33,"status":"energised","commissioned":1262304000}}
{"object":"feeder","key":"FDR-ELM-01","marking":"internal","purposes":["operations","asset-management"],"properties":{"name":"Elm Road 11 kV feeder 1","customers":1840}}
{"object":"transformer","key":"TX-40117","marking":"internal","purposes":["operations","asset-management"],"properties":{"serial":"TX-40117","rating_kva":500,"load_fraction":0.71,"oil_temp_c":58.5,"in_service":true,"status":"normal"}}
{"link":"feeder_from","from":"FDR-ELM-01","to":"SS-ELM"}
{"link":"transformer_on","from":"TX-40117","to":"FDR-ELM-01"}
```

### Object records

| Key | Required | Meaning |
|---|---|---|
| `object` | yes | An object type `id` from your ontology. |
| `key` | yes | Your identifier for this object, unique within its type. Letters, digits, `_` and `-` only, so no spaces, slashes or dots. The object's id becomes `<type>/<key>`, e.g. `transformer/TX-40117`. |
| `properties` | no | Values for declared properties, typed as in the table above. Every `required` property must be present. |
| `marking` | yes | Sensitivity band: `open`, `internal`, `restricted` or `confidential`. |
| `compartments` | no | Extra need-to-know labels, e.g. `["GRID-OPS"]`. A reader must hold all of them. |
| `purposes` | yes, at least one | Why the data may be read, e.g. `["operations"]`. A reader must hold one of them. |

> **Important:** in this version, every login is issued for the purpose `operations` (`LOGIN_PURPOSE` in [`services/http-gateway/src/lib.rs`](../../services/http-gateway/src/lib.rs)). An object whose `purposes` do not include `operations` imports without complaint but **cannot be read by anyone through the gateway**. Include `operations` in every record's `purposes`, and in every action's, until logins can carry other purposes.

### Link records

| Key | Meaning |
|---|---|
| `link` | A link type `id` from your ontology. |
| `from` | The **key** (not the full id) of an object of the link type's `from` type. |
| `to` | The key of an object of the link type's `to` type. |

Both ends must exist, either earlier or later in the same file or already in the enclosure. So you can add links between existing objects in a later file on their own.

### Converting from a spreadsheet or database export

Any tool that writes one JSON object per line will do. [`examples/utility/csv_to_jsonl.py`](examples/utility/csv_to_jsonl.py) is a short Python script with no dependencies. It turns [`transformers.csv`](examples/utility/transformers.csv) into transformer objects and their feeder links:

```bash
python docs/guides/examples/utility/csv_to_jsonl.py docs/guides/examples/utility/transformers.csv > transformers.jsonl
```

The points that catch most converters:

- **Numbers must be JSON numbers.** `"rating_kva":"500"` is refused because a quoted number is a string; write `"rating_kva":500`.
- **Leave empty cells out.** `null` is refused; an absent optional property is fine.
- **Booleans are `true`/`false`**, not `"yes"`, `1` or `"TRUE"`.
- **Dates become Unix seconds**, e.g. in Python `int(datetime(2026, 1, 1, tzinfo=timezone.utc).timestamp())`.
- **Keys must match `^[A-Za-z0-9_-]+$`.** Replace spaces, slashes and dots in your source identifiers.

## 5 — Import

With the gateway stopped:

```bash
./target/release/mg-admin import --state-dir ./grid --file docs/guides/examples/utility/network.jsonl
```

```text
imported 7 object(s) and 4 link(s) into ./grid
```

The import is **all or nothing**. Every line is checked against the ontology, and then against what the enclosure already holds, before anything is written. The first problem refuses the whole file, names the line or object at fault, and writes nothing:

```text
mg-admin: import refused: line 1 (transformer/TX-9): "rating_kva" must be an integer, not "500"
mg-admin: import refused: line 1 (transformer/TX-9): required property "rating_kva" is missing
mg-admin: import refused: substation/SS-ELM is already in the enclosure; an import never replaces an object
```

Fix the line and run the same command again. Each successful import is recorded in the enclosure's signed audit ledger before it is written.

You can also check files before you import, with any JSON Schema (draft 2020-12) validator against the two published schemas. A schema checks a file's shape only. Whether a type or property is declared, values match their declared types, and keys are unique and links resolve all depend on your ontology, and only `mg-admin import` checks those. Treat it as the final word.

### Adding more data later

- **New objects:** put them in a new file and import it. Only objects that are not already in the enclosure may appear.
- **New links between existing objects:** a file of link records alone is fine.
- **Changing or removing an object:** an import cannot do this. It only adds. Use an action for the properties actions cover, or see [Keeping data in sync](#keeping-data-in-sync).
- **Changing the ontology:** edit `ontology.json` and restart the gateway. Adding types and optional properties is the safe change; changing or removing ones your data already uses is not covered by this guide.

## 6 — Allow the actions

The starter `policy.json` lets role `operator` `Read` and `Apply`. Each action also needs a rule granting its **`name`**, otherwise it is refused. [`examples/utility/policy.json`](examples/utility/policy.json) adds one for each example action:

```json
{
  "version": 1,
  "rules": [
    { "id": "operator-read",    "role": "operator", "action": "Read",              "resource": "*", "effect": "permit" },
    { "id": "operator-apply",   "role": "operator", "action": "Apply",             "resource": "*", "effect": "permit" },
    { "id": "operator-flag",    "role": "operator", "action": "flagTransformer",   "resource": "*", "effect": "permit" },
    { "id": "operator-isolate", "role": "operator", "action": "isolateSubstation", "resource": "*", "effect": "permit" }
  ]
}
```

```bash
cp docs/guides/examples/utility/policy.json ./grid/policy.json
```

## 7 — Enrol a reader

```bash
read -rs PASSWORD
printf '%s\n' "$PASSWORD" | ./target/release/mg-admin user add --state-dir ./grid \
  --subject sam --roles operator --clearance internal --purposes operations
unset PASSWORD
```

A user sees an object only when **all** of these hold:

- their `--clearance` is at or above the object's `marking` (`open` < `internal` < `restricted` < `confidential`);
- they hold every one of the object's `compartments` (`--compartments GRID-OPS,...`);
- the object's `purposes` include `operations` (see [the note above](#object-records)).

With the command above, `sam` sees everything in the example except substation `SS-QUAY`, which is `restricted` and in compartment `GRID-OPS`. Objects a user may not see are not just hidden from results. They are reported exactly as if they did not exist. More enrolment options, including TOTP, are in the README's [Enrol a user](../../README.md#3--enrol-a-user).

## 8 — Serve and use it

```bash
./target/release/http-gateway --state-dir ./grid --allow-origin http://localhost:5173
```

```text
http-gateway: serving the enclosure in ./grid — 1 principal(s) enrolled, 0 administrator(s), 3 object type(s) and 2 action(s) registered, ...
```

Log in as `sam`, with the password you enrolled, from the Midgard console (see the README's [Start the Midgard console](../../README.md#4--start-the-midgard-console)) to browse the types, objects and links. Or use the HTTP API directly:

```bash
read -rs PASSWORD
TOKEN=$(curl -s -X POST http://127.0.0.1:7710/api/v1/login -H 'content-type: application/json' \
  -d "{\"username\":\"sam\",\"password\":\"$PASSWORD\"}" | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')
unset PASSWORD
AUTH=(-H "authorization: Bearer $TOKEN" -H 'content-type: application/json')
```

**List the object types** with their counts, links and actions:

```bash
curl -s "${AUTH[@]}" http://127.0.0.1:7710/api/v1/object-types
```

**Query a type**, optionally filtering on property equality. Filter values are always written as strings and are read against the property's type, so `"500"` matches the integer `500`. Several filters must all match. `maxResults` defaults to 5 and is capped at 500.

```bash
curl -s -X POST "${AUTH[@]}" http://127.0.0.1:7710/api/v1/query \
  -d '{"objectType":"transformer","filter":{"rating_kva":"500","in_service":"true"},"maxResults":50}'
```

```json
{"results":[{"id":"transformer/TX-40117","typeId":"transformer","propertyValues":{"in_service":"true","last_inspected":"1767225600","load_fraction":"0.71","oil_temp_c":"58.5","rating_kva":"500","serial":"TX-40117","status":"normal"},"markings":[{"id":"mk-internal","name":"INTERNAL","level":"internal","compartments":[]}]}],"auditId":"AUD-…"}
```

A filter on a property the type does not declare returns no results rather than an error, so check spelling if a query comes back empty.

**Walk the links** out from an object, up to `hops` steps (default 2) in either direction:

```bash
curl -s -X POST "${AUTH[@]}" http://127.0.0.1:7710/api/v1/search-around \
  -d '{"seedId":"substation/SS-ELM","hops":2}'
```

This returns the substation, its two feeders at hop 1, and the transformers on them at hop 2, with the edges between them. Use `"seedId":"transformer/*"` to start from every transformer you can see.

**Run an action**, by its `id` (or `title`), on one object or on `"<type>/*"`:

```bash
curl -s -X POST "${AUTH[@]}" http://127.0.0.1:7710/api/v1/actions \
  -d '{"actionType":"flag_transformer","params":{"target":"transformer/TX-40118"}}'
# {"auditId":"AUD-14","applied":true}
```

A human-gated action without a `humanGateToken` is refused with `Human gate required for action "Isolate substation"`. An action whose marking, compartments or purposes the caller does not hold is refused with `denied by policy`. Every read and every action, allowed or refused, is written to the audit ledger; `GET /api/v1/audit` lists it.

---

## Keeping data in sync

`mg-admin import` is for loading data you hold as files, and it only ever adds. If your data changes over time, the Forge worker can re-read a source on a schedule and create, update or link objects through the running gateway. Its sources are a CSV file, a PostgreSQL query, an S3 prefix or a Kafka topic. Its `jobs.json` maps source columns onto the same ontology you wrote in step 3. See [`deploy/compose/forge/jobs.json`](../../deploy/compose/forge/jobs.json) for an example, the format notes at the top of [`forge/worker/src/lib.rs`](../../forge/worker/src/lib.rs), and the README's [Run the services with docker compose](../../README.md#run-the-services-with-docker-compose). Unlike the ontology and import formats, `jobs.json` has no published JSON Schema yet.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `... is in use by http-gateway` from `mg-admin` | The gateway holds the state directory. Stop it first. If it was killed outright, it leaves `midgard.lock` behind. On Linux the next run takes the lock over; on other systems, delete the lock file once you are sure nothing is running. |
| Import succeeds but a query returns nothing | The objects' `purposes` lack `operations`, the user's clearance is below the `marking`, the user lacks a compartment, or a filter names a property that does not exist. |
| `unknown action type: flagTransformer` | Actions are called by `id` or `title`, not `name`. |
| `denied by policy` on an action | No `policy.json` rule grants the action's `name` to the user's role, or the user lacks the action's marking, compartment or purpose. |
| Gateway will not start after editing `ontology.json` | The error names the problem; compare with the failing examples in [`contracts/fixtures/ontology/`](../../contracts/fixtures/ontology/). |
| `"x" must be an integer, not "12"` | The value is a quoted string. Write the number without quotes. |

## Limits

These are the limits of the file-based path as it stands. The README's [Limits of a real enclosure today](../../README.md#limits-of-a-real-enclosure-today) and the [claims ledger](../compliance/CLAIMS.md) have the full, current picture.

- Imports add only; they never update or delete.
- Ontology and policy changes take effect when the gateway restarts.
- Login tokens carry the purpose `operations` only.
- Property types are limited to `text`, `int`, `float`, `bool` and `timestamp`, and action effects to text properties.
- The importer reads the whole file into memory and writes it in one go. It has not been measured on very large files; split very large loads into several files.
