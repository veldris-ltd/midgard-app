---
name: midgard-design
description: Use this skill to generate well-branded interfaces and assets for the Midgard platform suite (Midgard core, Forge, Mimir, Bifrost, Heimdall), either for production or throwaway prototypes/mocks/etc. Contains essential design guidelines, colors, type, fonts, assets, and UI kit components for prototyping.
user-invocable: true
---

Read the README.md file within this skill, and explore the other available files.

Key facts: dark cryptographic-terminal aesthetic (Veldris lineage); one accent hue per product applied via `.theme-midgard` / `.theme-forge` / `.theme-mimir` / `.theme-bifrost` / `.theme-heimdall` scope classes over semantic `--accent-*` tokens; Oxanium / Space Grotesk / JetBrains Mono; classification banners, markings, provenance tags (`[DD-1]`) as house devices; no logos exist, product names render in plain type (ProductMark); never emoji.

If creating visual artifacts (slides, mocks, throwaway prototypes, etc), copy assets out and create static HTML files for the user to view. If working on production code, you can copy assets and read the rules here to become an expert in designing with this brand.
If the user invokes this skill without any other guidance, ask them what they want to build or design, ask some questions, and act as an expert designer who outputs HTML artifacts _or_ production code, depending on the need.
