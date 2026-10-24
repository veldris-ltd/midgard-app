# Midgard design guide for coding agents

Instructions for an AI coding agent that generates interfaces and assets for the Midgard suite (Midgard core, Forge, Mimir, Bifrost, Heimdall), for production code or for throwaway prototypes and mocks. Read [`readme.md`](readme.md) in this directory first, then explore the tokens, components and UI kits it points to.

Key facts: dark cryptographic-terminal aesthetic (Veldris lineage); one accent hue per product applied via `.theme-midgard` / `.theme-forge` / `.theme-mimir` / `.theme-bifrost` / `.theme-heimdall` scope classes over semantic `--accent-*` tokens; Oxanium / Space Grotesk / JetBrains Mono; classification banners, markings, provenance tags (`[DD-1]`) as house devices; no logos exist, product names render in plain type (ProductMark); never emoji.

For visual artefacts (slides, mocks, throwaway prototypes), copy assets out and create static HTML files to view. For production code, copy assets as needed and follow the rules here and in `readme.md`.

Without further guidance, ask what is to be built or designed, ask the questions that settle scope, and act as an expert designer who outputs HTML artefacts or production code, depending on the need.
