"""Turn a CSV of transformers into Midgard import records (JSON Lines).

    python csv_to_jsonl.py transformers.csv > transformers.jsonl

Each row becomes one transformer object and, when it names a feeder, one
`transformer_on` link. Empty cells are left out rather than sent as null,
because the importer refuses null property values.
"""

import csv
import json
import sys

MARKING = "internal"
PURPOSES = ["operations", "asset-management"]

# Property name -> function turning the CSV text into the JSON value its
# ontology type needs: int for `int`, float for `float`, bool for `bool`.
COLUMNS = {
    "serial": str,
    "rating_kva": int,
    "load_fraction": float,
    "oil_temp_c": float,
    "in_service": lambda s: {"true": True, "false": False}[s.strip().lower()],
}


def main(path):
    with open(path, newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            properties = {
                name: convert(row[name])
                for name, convert in COLUMNS.items()
                if row.get(name, "").strip() != ""
            }
            record = {
                "object": "transformer",
                "key": row["serial"],
                "marking": MARKING,
                "purposes": PURPOSES,
                "properties": properties,
            }
            print(json.dumps(record, separators=(",", ":")))
            if row.get("feeder", "").strip():
                link = {"link": "transformer_on", "from": row["serial"], "to": row["feeder"]}
                print(json.dumps(link, separators=(",", ":")))


if __name__ == "__main__":
    main(sys.argv[1])
