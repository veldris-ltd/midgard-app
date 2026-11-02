# Data-protection screening: published sample datasets

This is a **screening record, not a full data protection impact assessment (DPIA)**.
It records what personal data the sample datasets published in [`samples/`](../../samples/)
may hold, the basis on which Veldris publishes them, and what was done about it. Whether
a full DPIA is needed is decided by the **Veldris Data Protection Officer**, on this
record; until that decision is recorded below, the screening stands as the assessment.
[VLD-ANB-MIDGARD-0074]

| | |
|---|---|
| Screened on | 2 November 2026 |
| Screened by | Security and compliance lead (role) |
| Decides whether a full DPIA is required | Data Protection Officer (role) |
| Full-DPIA decision | Not yet recorded |
| Next review | 2 May 2027, or earlier when a sample is added or refreshed |

## Purpose of the samples

The samples are recorded, real-world public data for evaluating Midgard with something
other than fictional rows: the operating picture, Forge ingestion and Heimdall feeds
([samples/README.md](../../samples/README.md)). Nobody's personal data is needed for
that purpose; where a source carries it, it is incidental to the feed.

## Data categories per file

The screening of each published file is recorded in
[`samples/catalogue.json`](../../samples/catalogue.json) as a `data_protection` object
(`personal_data` is `none`, `possible` or `yes`, with the basis, date, screening role and
notes) and shown in the "Personal data" column of
[`samples/NOTICE.md`](../../samples/NOTICE.md). `python3 -I samples/convert.py check`
fails when a published entry has no screening. Files kept out of the repository
(`publish: false`) are not screened here: they are not redistributed.

`python3 -I samples/convert.py screen` lists, for every file, the field names that could
identify a natural person (`name`, `callSign`, `mmsi`, `email`, `phone`, `address`,
`registration`, `plate`). No file holds an e-mail address, telephone number, postal
address, vehicle registration or number plate field. A `name` field elsewhere names a
place, line, airfield, fuel, installation, event or region, not a person.

| Files | Personal data | Why |
|-------|---------------|-----|
| `3-heimdall-feed-snapshots/digitraffic_vessels.json` | possible | AIS static data: MMSI, name, call sign, IMO number, destination, ETA, dimensions. Small craft pseudonymised (below) |
| `4-heimdall-observations/digitraffic-ais.json` | yes | 500 AIS position reports: MMSI, name, call sign, position and time |
| `3-heimdall-feed-snapshots/gdelt.json`, `4-heimdall-observations/gdelt-news.json` | possible | Sample news headlines and article links; a headline can name a person |
| Every other published file (weather, seismic, flood, grid, transport, aviation, offshore and censorship data) | none | Places, readings and events; camera files hold camera locations and links, not images |

## AIS vessel snapshot: the small-craft heuristic

Commercial ships carry an IMO number and are operated by companies; their names and
positions are published worldwide for safety of navigation. A craft with no IMO number
and a hull under 24 metres (the conventional small-craft threshold) is the kind most
likely to be owned and sailed by a private person, so its name, call sign and MMSI can
identify an owner and, with a position, their movements. The heuristic is a screening
aid, not a legal test: the decision and its basis are Veldris's.

The hull length is `referencePointA + referencePointB` (antenna to bow plus antenna to
stern). A record that reports no dimensions counts as under 24 m.

`python3 -I samples/convert.py screen --samples samples` on the snapshot as committed
before this screening (dated 22 October 2026):

| | Records |
|---|---|
| Vessels in the snapshot | 1,065 |
| Without an IMO number | 319 |
| Small craft: no IMO number and under 24 m | 212 |
| of which report no dimensions | 17 |

By declared ship type the 212 are mostly working craft (127 pilot, search and rescue,
tug, port and law-enforcement vessels; 19 passenger; 15 cargo or tanker; 8 towing,
dredging, diving or military), with 3 sailing or pleasure craft and 40 of another or
unreported type. Counts only: this record names no vessel.

## Decision

**Filtered.** The count is above zero, so `convert.py screen --apply` was run: the 212
small-craft records in `digitraffic_vessels.json` lost their `name` and `callSign`, and
their MMSI (the key, and the record's own `mmsi` field) was replaced by
`small-craft-<n>`, numbered in ascending MMSI order. The run is deterministic and a
second run changes nothing. Destination, ETA, draught, type and dimensions are kept: they
describe a voyage, not a person, once the vessel is no longer named. The remaining 853
records are retained on the basis below.

**Retained, pending the DPO's decision:** `4-heimdall-observations/digitraffic-ais.json`
still holds MMSI, name, call sign and position for each of its 500 records, 110 of which
are small craft that the snapshot now pseudonymises. Position with time is the more
sensitive combination, and an MMSI there can be linked back to a pseudonymised snapshot
record by its static fields. The pseudonymisation this package specifies covers the
snapshot only; whether to pseudonymise or remove the small craft from the observations
too is for the Data Protection Officer to decide with the full-DPIA question.

**Basis for publishing.** The AIS data is published by Fintraffic (digitraffic.fi) as open
data under CC BY 4.0. Veldris publishes the samples in its legitimate interest of letting
evaluators assess Midgard's maritime picture with real traffic; pseudonymising the small
craft keeps that purpose while removing the records most likely to relate to a private
individual. The GDELT headlines are public reporting published for free use with
attribution, kept for evaluating news surge detection.

## Retention

The samples are a snapshot exported on 22 October 2026. They are not updated in place:
a newer snapshot comes only from re-running the export pipeline, and a refreshed AIS
snapshot is screened and pseudonymised again (`convert.py screen --apply`) before it is
committed. Superseded snapshots remain in the repository's history.

## Operator responsibility

The samples are for evaluation. An operator who loads them, or feeds live AIS or news
data into a deployment, is the controller for that deployment and carries the duties in
[DATA_RESIDENCY.md](DATA_RESIDENCY.md), "Operator responsibilities", and the retention
limits in [RETENTION.md](RETENTION.md).
