# Fonts

The design system's three typefaces, self-hosted so a console renders its own
type with no network. `../tokens/fonts.css` declares them. Each is the
unmodified variable-weight build from [google/fonts](https://github.com/google/fonts),
redistributed under the SIL Open Font License 1.1 with its licence text beside it.
This is the only tracked copy: the pages under `docs/design` load these files, and the
launch site's builder copies them into `docs/launch/site/assets/fonts`. The Mac app's
home page uses them through a symlink (`apps/mac/Resources/home/fonts`); the app build
copies the files into the bundle.

| File | Family | Weight axis | Licence |
|---|---|---|---|
| `Oxanium.ttf` | Oxanium | 200–800 | `Oxanium-OFL.txt` |
| `SpaceGrotesk.ttf` | Space Grotesk | 300–700 | `SpaceGrotesk-OFL.txt` |
| `JetBrainsMono.ttf` | JetBrains Mono | 100–800 | `JetBrainsMono-OFL.txt` |

## Sources

Fetched from the `main` branch of google/fonts: the fonts on 2026-09-10, the
licences on 2026-09-11.

| File | Source | sha256 |
|---|---|---|
| `Oxanium.ttf` | `ofl/oxanium/Oxanium[wght].ttf` | `2ce01d946e1e1ffc8d7eecfffbda8623bedd63eaf811a20488c4b69af45babb0` |
| `SpaceGrotesk.ttf` | `ofl/spacegrotesk/SpaceGrotesk[wght].ttf` | `acad6de1fc93436f5c0f1f4137751ef04f1aea3063e7036535970ffcfbd79f72` |
| `JetBrainsMono.ttf` | `ofl/jetbrainsmono/JetBrainsMono[wght].ttf` | `48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda` |
| `Oxanium-OFL.txt` | `ofl/oxanium/OFL.txt` | `fe17c0f2581d71b4e1ea7e636e7f4877c29223e11bb1dd1a871e8c3f2a86336b` |
| `SpaceGrotesk-OFL.txt` | `ofl/spacegrotesk/OFL.txt` | `564ce565c371c5e5bbf286006565a7c9aa55a9f56e7ca58d56e05d649dd61a72` |
| `JetBrainsMono-OFL.txt` | `ofl/jetbrainsmono/OFL.txt` | `b2fe5e8987594e9ffd1d2ca52a2f5d73eb8335243893c5d6254b5ad69269591d` |

Paths are relative to `https://raw.githubusercontent.com/google/fonts/main/`.
To replace a face, update the file, its row here, and `../tokens/fonts.css`
(the `font-weight` range must match the file's weight axis).
