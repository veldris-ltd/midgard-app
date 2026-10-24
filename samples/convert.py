#!/usr/bin/env python3
"""Turn the Midgard sample datasets into what Midgard ingests.

    convert.py check        --samples DIR
    convert.py import       --samples DIR [--datasets a,b]   > records.jsonl
    convert.py forge        --samples DIR --out DIR [--datasets a,b]
    convert.py observations --samples DIR --feeds a,b        > observations.tsv
    convert.py ontology     --samples DIR > ontology.json
    convert.py merge        FILE FILE... > merged.json
    convert.py feeds        --samples DIR [--families a,b]
    convert.py credits      --samples DIR [--datasets a,b] [--feeds a,b]
    convert.py notice       > NOTICE.md

`import` writes JSON Lines for `mg-admin import` (docs/guides/IMPORT_YOUR_OWN_DATA.md):
one object record per dataset row, then the link records whose both ends exist.
`forge` writes, for the Forge worker, one CSV per dataset and a jobs.json naming only
those CSVs. `observations` writes one tab-separated line per Heimdall observation:
feed, kind, entity, observed-at, attributes; --budget-kb trims the largest feeds so the
fused picture stays readable. `ontology` writes the configuration's ontology with
only the keys this build's gateway reads (contracts/schema/ontology.v1.schema.json),
naming on stderr each key it leaves out. `merge` joins ontology or policy files,
refusing an id that two files define differently. `feeds` lists, from
catalogue.json, each observation feed's family and whether DIR holds it; with
--families, only the names of the feeds DIR holds in those families. `credits` prints
the attribution each chosen dataset and feed's licence requires. `check` validates the datasets
against the configuration's jobs and ontology.

Each conversion is driven by 1-configuration/forge/jobs.json: a dataset is ingested
with its job's object type, key column, marking, purposes and property mapping, so
the import route and the Forge route produce the same objects. Standard library
only. [FR-F-1] [FR-H-3] [RC-33]
"""

import argparse
import csv
import json
import math
import os
import re
import sys

KEY = re.compile(r"^[A-Za-z0-9_-]+$")


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def configuration(samples):
    jobs = {j["id"]: j for j in load(os.path.join(samples, "1-configuration/forge/jobs.json"))["jobs"]}
    ontology = load(os.path.join(samples, "1-configuration/midgard/ontology.json"))
    types = {t["id"]: {p["name"]: p for p in t["properties"]} for t in ontology["objectTypes"]}
    links = {l["id"]: l for l in ontology.get("linkTypes", [])}
    return jobs, types, links


def datasets(samples, only):
    folder = os.path.join(samples, "2-forge-datasets")
    names = sorted(f[:-5] for f in os.listdir(folder) if f.endswith(".json"))
    if only:
        wanted = [n.strip() for n in only.split(",") if n.strip()]
        names = [n for n in names if n in wanted]
    return {n: load(os.path.join(folder, n + ".json")) for n in names}


def regions_dataset(job_for, data):
    """The 'regions' dataset has no job: give it one, keyed and marked like the rest."""
    if "regions" in data and "regions" not in job_for:
        job_for["regions"] = {
            "id": "regions",
            "source": {"file": "regions.csv", "header": True,
                       "columns": [{"name": "key", "type": "text"}, {"name": "name", "type": "text"},
                                   {"name": "code", "type": "text"}]},
            "objects": {"type": "region", "key": "key", "marking": "internal",
                        "purposes": ["operations"], "properties": {"name": "name", "code": "code"}},
            "every": 3600,
        }


def plan(samples, only):
    """The datasets to ingest, their jobs, and the links that can resolve."""
    jobs, types, link_types = configuration(samples)
    data = datasets(samples, only)
    if any(l.get("to") == "region" or l.get("from") == "region" for l in link_types.values()):
        # The original zip has no regions.json: it was written for this repository.
        path = os.path.join(samples, "2-forge-datasets/regions.json")
        if not os.path.exists(path):
            path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "2-forge-datasets/regions.json")
        data.setdefault("regions", load(path))
    regions_dataset(jobs, data)
    present = {jobs[n]["objects"]["type"] for n in data if n in jobs}
    kept, dropped = {}, []
    # Regions first, so a link to one resolves on the Forge worker's first run.
    for name in sorted(data, key=lambda n: n != "regions"):
        if name not in jobs:
            raise SystemExit(f"convert: dataset '{name}' has no job in 1-configuration/forge/jobs.json")
        job = json.loads(json.dumps(jobs[name]))
        links = []
        for link in job.get("links", []):
            target = link_types[link["type"]]["to"]
            if target in present:
                links.append(link)
            else:
                dropped.append(f"{name}: link {link['type']} (no '{target}' objects among these datasets)")
        if links:
            job["links"] = links
        else:
            job.pop("links", None)
        kept[name] = job
    for line in dropped:
        print(f"convert: skipping {line}", file=sys.stderr)
    return kept, data, types


def missing(v):
    """None, an empty string, or a NaN or infinity (the source JSON has a few NaNs, which
    no JSON parser downstream accepts) all mean no value."""
    return v is None or (isinstance(v, str) and v.strip() == "") or (isinstance(v, float) and not math.isfinite(v))


def value(kind, v):
    if missing(v):
        return None
    if kind == "int" or kind == "timestamp":
        return int(v)
    if kind == "float":
        return float(v)
    if kind == "bool":
        return bool(v)
    return str(v)


def cmd_import(args):
    jobs, data, types = plan(args.samples, args.datasets)
    out = sys.stdout
    keys = {}
    for name, job in jobs.items():
        o = job["objects"]
        props = types[o["type"]]
        for row in data[name]["rows"]:
            properties = {}
            for prop, column in o["properties"].items():
                v = value(props[prop]["type"], row.get(column))
                if v is not None:
                    properties[prop] = v
            record = {"object": o["type"], "key": str(row[o["key"]]), "marking": o["marking"],
                      "purposes": o["purposes"], "properties": properties}
            if o.get("compartments"):
                record["compartments"] = o["compartments"]
            keys.setdefault(o["type"], set()).add(record["key"])
            out.write(json.dumps(record, ensure_ascii=False) + "\n")
    for name, job in jobs.items():
        o = job["objects"]
        for link in job.get("links", []):
            for row in data[name]["rows"]:
                frm, to = row.get(link["from"]), row.get(link["to"])
                if frm is None or to is None:
                    continue
                out.write(json.dumps({"link": link["type"], "from": str(frm), "to": str(to)}) + "\n")


def cmd_forge(args):
    jobs, data, _ = plan(args.samples, args.datasets)
    os.makedirs(args.out, exist_ok=True)
    for name, job in jobs.items():
        columns = [c["name"] for c in job["source"]["columns"]]
        with open(os.path.join(args.out, job["source"]["file"]), "w", newline="", encoding="utf-8") as f:
            w = csv.writer(f, lineterminator="\n")
            w.writerow(columns)
            for row in data[name]["rows"]:
                cells = []
                for c in columns:
                    v = row.get(c)
                    if missing(v):
                        cells.append("")
                    elif isinstance(v, bool):
                        cells.append("true" if v else "false")
                    else:
                        cells.append(str(v).replace("\r", " ").replace("\n", " "))
                w.writerow(cells)
    with open(os.path.join(args.out, "jobs.json"), "w", encoding="utf-8") as f:
        json.dump({"version": 1, "jobs": list(jobs.values())}, f, indent=2)
        f.write("\n")
    print(f"convert: wrote {len(jobs)} job(s) and their CSV files to {args.out}", file=sys.stderr)


def attrs(a):
    """heimdall-source --attrs: k=v pairs split on commas, so no value may hold one."""
    pairs = []
    for k, v in a.items():
        if missing(v) or isinstance(v, (dict, list)) or not KEY.match(k):
            continue
        if isinstance(v, bool):
            v = "true" if v else "false"
        v = re.sub(r"[\t\r\n]+", " ", str(v)).replace(",", ";").strip()
        if v:
            pairs.append(f"{k}={v}")
    return ",".join(pairs)


def entity_cost(line):
    """A generous estimate of the bytes an entity adds to Heimdall's picture JSON: its
    attributes quoted as JSON, its id and kind twice over, and the fixed fields."""
    _, kind, entity, _, text = line
    return int(len(text) * 1.3) + 4 * text.count("=") + 2 * (len(entity) + len(kind)) + 320


def budgeted(per_feed, budget):
    """Share the budget between feeds, smallest first, so a small feed keeps everything
    and the large ones are trimmed evenly. Returns the entities kept per feed."""
    costs = {}
    for feed, lines in per_feed.items():
        seen = {}
        for line in lines:
            seen[line[2]] = max(seen.get(line[2], 0), entity_cost(line))
        costs[feed] = list(seen.items())
    kept, left = {}, budget
    order = sorted(costs, key=lambda f: sum(c for _, c in costs[f]))
    for i, feed in enumerate(order):
        share = left // (len(order) - i)
        taken, used = set(), 0
        for entity, cost in costs[feed]:
            if used + cost > share:
                break
            taken.add(entity)
            used += cost
        kept[feed] = taken
        left -= used
        if len(taken) < len(costs[feed]):
            print(f"convert: {feed}: keeping {len(taken)} of {len(costs[feed])} entities to fit the picture budget",
                  file=sys.stderr)
    return kept


def cmd_observations(args):
    folder = os.path.join(args.samples, "4-heimdall-observations")
    wanted = [f.strip() for f in args.feeds.split(",") if f.strip()]
    per_feed = {}
    for feed in wanted:
        path = os.path.join(folder, feed + ".json")
        if not os.path.exists(path):
            print(f"convert: no observations for feed '{feed}' in {folder}", file=sys.stderr)
            continue
        doc = load(path)
        # Most files hold their observations under "sample", the adsb-fi ones under
        # "observations". The feed is named after the file: that is the service each
        # feed's mesh identity is issued for, and two files share the feed id adsb-fi.
        for o in doc.get("sample", doc.get("observations", [])):
            if not (o.get("entity") and o.get("kind")):
                continue
            line = [feed, o["kind"], str(o["entity"]), str(int(o.get("observedAt") or 0)), attrs(o.get("attrs", {}))]
            if any("\t" in c or "\n" in c for c in line[:4]):
                continue
            per_feed.setdefault(feed, []).append(line)
    kept = budgeted(per_feed, args.budget_kb * 1024) if args.budget_kb else None
    for feed, lines in per_feed.items():
        for line in lines:
            if kept is None or line[2] in kept[feed]:
                print("\t".join(line))


# The keys contracts/schema/ontology.v1.schema.json allows. The samples were recorded on
# a build that also reads `retention` on an object type; this one does not.
ONTOLOGY_KEYS = {
    "objectTypes": {"id", "title", "properties"},
    "linkTypes": {"id", "name", "from", "to", "cardinality"},
    "actions": {"id", "name", "title", "objectType", "humanGated", "marking", "compartments", "purposes", "effect"},
}


def cmd_ontology(args):
    doc = load(os.path.join(args.samples, "1-configuration/midgard/ontology.json"))
    out = {"version": doc.get("version", 1)}
    for section, allowed in ONTOLOGY_KEYS.items():
        out[section] = []
        for item in doc.get(section, []):
            extra = sorted(set(item) - allowed)
            for key in extra:
                print(f"convert: {section} '{item['id']}': leaving out '{key}', which this build does not read",
                      file=sys.stderr)
            out[section].append({k: v for k, v in item.items() if k in allowed})
    # This build's actions change text properties only.
    kinds = {t["id"]: {p["name"]: p["type"] for p in t["properties"]} for t in out["objectTypes"]}
    kept = []
    for action in out["actions"]:
        prop = action["effect"]["property"]
        if kinds.get(action["objectType"], {}).get(prop) != "text":
            print(f"convert: actions '{action['id']}': leaving out the action, because this build's actions "
                  f"change text properties only and '{prop}' is not one", file=sys.stderr)
            continue
        kept.append(action)
    out["actions"] = kept
    json.dump(out, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


def cmd_merge(args):
    merged, section = None, None
    for path in args.files:
        doc = load(path)
        if merged is None:
            merged = {"version": doc.get("version", 1)}
        for key in ("objectTypes", "linkTypes", "actions", "rules", "jobs"):
            if key not in doc:
                continue
            section = merged.setdefault(key, [])
            seen = {item["id"]: item for item in section}
            for item in doc[key]:
                if item["id"] in seen:
                    if seen[item["id"]] != item:
                        raise SystemExit(f"convert: {key} '{item['id']}' differs between files")
                    continue
                section.append(item)
                seen[item["id"]] = item
    json.dump(merged, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


def catalogue():
    return load(os.path.join(os.path.dirname(os.path.abspath(__file__)), "catalogue.json"))


def cmd_feeds(args):
    cat = catalogue()["observations"]
    folder = os.path.join(args.samples, "4-heimdall-observations")
    have = lambda f: os.path.exists(os.path.join(folder, f + ".json"))
    if args.families:
        wanted = {f.strip() for f in args.families.replace(" ", ",").split(",") if f.strip()}
        print(" ".join(f for f, e in cat.items() if e["family"] in wanted and have(f)))
        return
    for f, e in cat.items():
        print("\t".join([e["family"], f, "yes" if have(f) else "no", "yes" if e["publish"] else "no"]))


def cmd_credits(args):
    cat = catalogue()
    names = lambda text: [n.strip() for n in (text or "").replace(" ", ",").split(",") if n.strip()]
    lines, uncleared = [], []
    for section, chosen in (("forge_datasets", names(args.datasets)), ("observations", names(args.feeds))):
        for n in chosen:
            e = cat[section].get(n)
            if e is None:
                continue
            folder = "2-forge-datasets" if section == "forge_datasets" else "4-heimdall-observations"
            if not os.path.exists(os.path.join(args.samples, folder, n + ".json")):
                continue
            if e["publish"]:
                lines.append(f"- `{n}`: {e['credit']} ({e['licence']})")
            else:
                uncleared.append(f"- `{n}`: from your local copy; not cleared for redistribution ({e['excluded_because']}). Check its terms before any use beyond evaluation.")
    print("\n".join(lines + uncleared))


def cmd_notice(args):
    """NOTICE.md, rendered from catalogue.json so the two cannot disagree."""
    cat = catalogue()
    out = ["# Sample datasets: sources and licences", "",
           "<!-- Generated by `python3 samples/convert.py notice > samples/NOTICE.md` from catalogue.json. Do not edit. -->", "",
           cat["about"], "",
           f"The configuration in `1-configuration/` is the work of {cat['configuration']['credit']}, licensed {cat['configuration']['licence']}.", ""]
    sections = (("forge_datasets", "Forge datasets", "2-forge-datasets"),
                ("feed_snapshots", "Heimdall feed snapshots", "3-heimdall-feed-snapshots"),
                ("observations", "Heimdall observations", "4-heimdall-observations"))
    for key, title, folder in sections:
        rows = cat[key]
        out += [f"## {title} (`{folder}/`)", "", "| File | In this repository | Licence | Credit to keep |", "|---|---|---|---|"]
        for name, e in sorted(rows.items(), key=lambda kv: (not kv[1]["publish"], kv[0])):
            if e["publish"]:
                out.append(f"| `{name}.json` | yes | {e['licence']} | {e['credit']} |")
            else:
                out.append(f"| `{name}.json` | no: {e['excluded_because']} | | |")
        out.append("")
    out += ["Files marked 'no' are in midgard-sample-datasets.zip only. `mg-install` can load them from a local copy; "
            "check each source's terms before any use beyond evaluation. Cloudflare Radar and openAIP data are licensed "
            "CC BY-NC 4.0, for non-commercial use only.", ""]
    print("\n".join(out))


def cmd_check(args):
    jobs, data, types = plan(args.samples, None)
    problems = 0
    for name, job in jobs.items():
        o = job["objects"]
        props = types.get(o["type"])
        if props is None:
            print(f"{name}: object type '{o['type']}' is not in the ontology"); problems += 1; continue
        required = [p for p, d in props.items() if d.get("required")]
        columns = {c["name"] for c in job["source"]["columns"]}
        for prop, column in o["properties"].items():
            if prop not in props:
                print(f"{name}: property '{prop}' is not declared on '{o['type']}'"); problems += 1
            if column not in columns:
                print(f"{name}: column '{column}' is not a source column"); problems += 1
        for i, row in enumerate(data[name]["rows"]):
            k = row.get(o["key"])
            if k is None or not KEY.match(str(k)):
                print(f"{name} row {i}: key {k!r} is not letters, digits, '_' or '-'"); problems += 1
            for p in required:
                if value(props[p]["type"], row.get(o["properties"].get(p, p))) is None:
                    print(f"{name} row {i}: required '{p}' is empty"); problems += 1
        print(f"{name}: {len(data[name]['rows'])} rows -> {o['type']}")
    if problems:
        raise SystemExit(f"convert: {problems} problem(s)")
    print("convert: every dataset fits its job and the ontology")


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("check", "import", "forge", "observations", "ontology"):
        s = sub.add_parser(name)
        s.add_argument("--samples", default=os.path.dirname(os.path.abspath(__file__)))
        if name in ("import", "forge"):
            s.add_argument("--datasets", default="")
        if name == "forge":
            s.add_argument("--out", required=True)
        if name == "observations":
            s.add_argument("--feeds", required=True)
            s.add_argument("--budget-kb", type=int, default=0,
                           help="keep the estimated picture under this size: Heimdall returns the "
                                "whole picture in one PQ-mTLS frame of at most 1 MiB")
    fe = sub.add_parser("feeds")
    fe.add_argument("--samples", default=os.path.dirname(os.path.abspath(__file__)))
    fe.add_argument("--families", default="")
    cr = sub.add_parser("credits")
    cr.add_argument("--samples", default=os.path.dirname(os.path.abspath(__file__)))
    cr.add_argument("--datasets", default="")
    cr.add_argument("--feeds", default="")
    sub.add_parser("notice")
    m = sub.add_parser("merge")
    m.add_argument("files", nargs="+")
    args = p.parse_args()
    {"check": cmd_check, "import": cmd_import, "forge": cmd_forge,
     "observations": cmd_observations, "merge": cmd_merge, "ontology": cmd_ontology,
     "feeds": cmd_feeds, "credits": cmd_credits, "notice": cmd_notice}[args.cmd](args)


if __name__ == "__main__":
    main()
