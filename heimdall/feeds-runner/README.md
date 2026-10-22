# Heimdall live feeds runner

`feeds.py` fetches the live sources (aircraft, ships, weather, utilities, airspace warnings,
the Gulf watch and others), turns each into Heimdall observations and sends them through
`heimdall-source` under each feed's own mesh identity. Each source's docstring says what it
fetches, its licence and how often.

It runs from the enclosure home, `~/Desktop/Midgard/heimdall-poc/feeds`, where `feeds.py`,
`start.sh`, `stop.sh` and `cameras.example.json` are links to the files here. That folder
also holds what is never committed: `secrets/` (API keys and the Network Rail login, one file
each), `state/` (caches and `status.json`), `out/` (the last batch per feed) and the logs.

    bash ~/Desktop/Midgard/heimdall-poc/feeds/start.sh      # run in the background (feeds.log)
    bash ~/Desktop/Midgard/heimdall-poc/feeds/stop.sh
    ../.venv/bin/python feeds.py --once <source> ...         # one cycle of named sources

Rules, alert severities and expiry times for the kinds it sends are in
`heimdall-poc/state/{rules,alerts,ttl}.json`. While `tools/mac-app/record.py` records, the
`record_copy` hook copies every sent batch to the recorder (see record.py's docstring).
