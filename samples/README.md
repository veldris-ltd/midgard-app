# Midgard sample datasets

Recorded, real-world data for trying Midgard with something other than fictional rows: the configuration it was recorded with, Forge datasets, Heimdall feed snapshots and Heimdall observations. The data was exported on 22 October 2026 from a Midgard deployment fed by live public sources.

This folder holds the **openly licensed part** of the set. The full set, `midgard-sample-datasets.zip`, also holds sources whose terms do not allow publication in an open repository; `mg-install` and `convert.py` read the zip in the same way when you have a copy. Which file is which, and the credit each licence asks for, is in [NOTICE.md](NOTICE.md), rendered from [catalogue.json](catalogue.json).

| Folder | What it holds | Files here | In the full set |
|---|---|---|---|
| `1-configuration/` | The ontology, policies, Forge job definitions and Heimdall rules the data was recorded with | 8 | 8 |
| `2-forge-datasets/` | Forge source tables as typed JSON | 10 | 11, plus `regions.json` from here |
| `3-heimdall-feed-snapshots/` | The latest cached response from each live source, in that source's own format | 26 | 49 |
| `4-heimdall-observations/` | Up to 500 normalised Heimdall observations per feed, from a 5-minute recording | 10 | 25 |
| `catalogue.json` · `NOTICE.md` | Source, licence, credit, feed family and publication status of every file | | |
| `convert.py` | Turns the data into what Midgard ingests (Python 3.8+, standard library only) | | |

`regions.json` was written for this repository: four ISO 3166-2 regions, so the datasets' links to `region` resolve.

---

## Formats

### 1. Configuration

| File | Format | Read by this build |
|---|---|---|
| `midgard/ontology.json` | [ontology file v1](../contracts/schema/ontology.v1.schema.json): 22 object types, 13 link types, 18 actions | yes, through `convert.py ontology` (see [Differences](#differences-from-the-build-that-recorded-the-data)) |
| `midgard/policy.json` | policy file: roles `operator`, `forge-worker`, `heimdall-sync` | yes, as it is |
| `forge/jobs.json` | Forge jobs file (format at the top of `forge/worker/src/lib.rs`): 11 jobs reading CSV files | yes, through `convert.py forge` |
| `mimir/policy.json` | Mimir policy: adds a read-only `inference-observer` role | yes, as it is |
| `heimdall/policy.json` | Heimdall policy: adds history, alert and question actions | its `OpenChannel`, `Ingest` and `ReadPicture` rules |
| `heimdall/rules.json` · `alerts.json` · `ttl.json` | 84 classification rules, alert detectors, and time-to-live per entity kind | no: the build that recorded the data reads them; this one has no rules engine, alerting or expiry in `heimdall-feed` |

### 2. Forge datasets

One file per dataset. Each row's keys are the columns its Forge job names, and `key` is the object key:

```json
{ "dataset": "floods", "objectType": "flood_warning",
  "rows": [ { "key": "054WATBT1", "region": "GB-ENG", "name": "The tidal Rivers Bure, Ant and Thurne",
              "severity": "No longer in force", "severity_level": 3, "active": false,
              "county": "Norfolk", "river_or_sea": "Bure, Ant, Thurne", "is_tidal": true,
              "message": "Tide locking, ...", "time_raised": 1792153093, "time_changed": 1792153080,
              "ea_area": "East Anglia" } ] }
```

| Dataset | Object type | Rows | Source |
|---|---|---|---|
| `earthquakes` | `earthquake` | 209 | USGS, M2.5+ in the past day |
| `floods` | `flood_warning` | 12 | Environment Agency flood warnings |
| `grid_frequency` | `grid_frequency` | 1 | Elexon, GB system frequency |
| `grid_fuel` | `grid_fuel` | 20 | Elexon, generation by fuel type |
| `grid_intensity_linked` · `_other` | `grid_region` | 16 · 2 | NESO Carbon Intensity API, by region |
| `space_weather` | `space_weather` | 62 | NOAA SWPC planetary K-index |
| `tfl` | `tfl_line` | 20 | TfL line status |
| `weather` | `weather_report` | 72 | NOAA METAR observations |
| `regions` | `region` | 4 | written for this repository |
| `aircraft` (full set only) | `aircraft_live` | 275 | ADS-B aggregator |
| `air_quality` (full set only) | `air_quality_sensor` | 200 | Sensor.Community |

### 3. Heimdall feed snapshots

Each file is the last response a live source returned, cached in that source's own shape (`{"fetched": ..., "items": {...}}`, `{"at": ..., "by_icao": {...}}` and others). They are reference data: Heimdall does not ingest them directly. The same feeds, normalised, are in `4-heimdall-observations/`.

### 4. Heimdall observations

What a feed source submits to `heimdall-feed`: an entity key, a kind, a time and flat attributes.

```json
{ "feed": "gb-grid", "total_in_recording": 11,
  "sample": [ { "entity": "GB-GRID", "kind": "utility-power-grid", "observedAt": 1792691220,
                "attrs": { "lat": 54.3, "lon": -2.3, "name": "GB electricity grid", "frequency_hz": 50.108,
                           "demand_mw": 38037, "carbon_index": "high", "source": "elexon" } } ] }
```

The two `adsb-fi` files hold their list under `observations` instead of `sample`. Feeds belong to families (aviation, maritime, energy, water, transport, communications, hazards, information), which is how `mg-install` offers them.

---

## Load the data by hand

These steps follow [Import your own data](../docs/guides/IMPORT_YOUR_OWN_DATA.md), with `convert.py` producing the two files that guide has you write. Every command was run against this folder.

```bash
cargo build --release -p http-gateway -p mg-admin
./target/release/mg-admin init --state-dir ./enclosure

python3 samples/convert.py ontology > ./enclosure/ontology.json
cp samples/1-configuration/midgard/policy.json ./enclosure/policy.json

python3 samples/convert.py import > samples.jsonl
./target/release/mg-admin import --state-dir ./enclosure --file samples.jsonl
# imported 418 object(s) and 28 link(s) into ./enclosure
```

Then enrol a user with the purpose `operations` and clearance `internal` or higher, and serve it, as in steps 7 and 8 of the guide. The object-type view then shows 209 earthquakes, 12 flood warnings, 1 grid frequency reading, 20 fuel-type rows, 18 grid regions, 62 space weather periods, 20 TfL lines, 72 weather reports and 4 regions.

For the full set, unzip it and point `--samples` at the folder that holds `1-configuration`:

```bash
python3 samples/convert.py ontology --samples "Midgard sample datasets" > ./enclosure/ontology.json
python3 samples/convert.py import --samples "Midgard sample datasets" > samples.jsonl
```

`--datasets earthquakes,floods` limits an import to some datasets. `python3 samples/convert.py check` checks every dataset against its job and the ontology first.

### Through Forge instead

Forge reads CSV files on a schedule. `convert.py forge` writes one CSV per dataset and a jobs file naming them:

```bash
python3 samples/convert.py forge --out ./forge-samples
# jobs.json and earthquakes.csv, floods.csv, ... ready for forge-worker --jobs ./forge-samples/jobs.json
```

### Heimdall observations

`convert.py observations` writes one tab-separated line per observation (feed, kind, entity, observed-at, attributes), the arguments `heimdall-source` takes:

```bash
python3 samples/convert.py observations --feeds gb-grid,reservoirs --budget-kb 1100 > observations.tsv
```

Each feed needs its own mesh identity, issued with `mg-admin mesh issue --service <feed> --roles feed-source`, because Heimdall accepts an observation only for the feed its sender's certificate names. `mg-install` does all of this for you.

---

## Differences from the build that recorded the data

The data was recorded on a newer Midgard build than this repository. `convert.py` bridges the differences, and says so on its error output whenever it does:

| Difference | What `convert.py` does |
|---|---|
| `heimdall_track` and `heimdall_alert` carry a `retention` rule, which this gateway does not read | `ontology` leaves the key out. Objects of those types do not expire here. |
| The action `toggle_runway_status` changes a boolean; this build's actions change text properties only | `ontology` leaves that one action out |
| The `weather` job links each report to an `airport`, and no dataset holds airports | `import` and `forge` skip that link and say so. The full set's airport snapshot covers only 68 of the 72 reporting airports, and the gateway refuses a whole sync over one dangling link, so the link stays out rather than fail the job. |
| The original `air_quality.json` (full set) held one `NaN` (sensor SC-36399's humidity), which is not JSON and made the gateway refuse the dataset | Corrected at source to `null` in the full set; its `CORRECTIONS.md` records the change. Every route still treats a `NaN` or infinity as a missing value, in case another export carries one. |
| `heimdall-source --attrs` splits on commas and has no escape | `observations` replaces commas inside a value with semicolons, and leaves out nested values |
| The airport code `NAN` (Nadi, Fiji) in `airports-metar` was read by `heimdall-feed` as a floating-point NaN, and so shown as `null` | Fixed in `heimdall-feed` itself: only finite numbers become numbers, and `nan`, `inf` and `infinity` stay text |
| `heimdall-feed` returns the whole picture in one PQ-mTLS frame of at most 1 MiB | `observations --budget-kb` trims the largest feeds evenly so the picture stays readable; `mg-install` uses 1100, which measured 701 KiB with the full aviation and maritime feeds |

---

## Licences

The configuration and `regions.json` are Veldris Ltd's work under Apache-2.0. Every other file keeps its source's licence, and several require a credit wherever the data is shown: see [NOTICE.md](NOTICE.md). `mg-install` writes the credits for what it loads into the install report.

The classification is conservative and is not legal advice: a file is here only if its source states an open licence that allows redistribution. Cloudflare Radar and openAIP data (full set only) are licensed CC BY-NC 4.0, for non-commercial use only.
