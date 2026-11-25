# grogu

A standalone Rust binary that paints a coherent palette across the whole
desktop in one shot:

| target | what it writes |
| --- | --- |
| **[Noctalia shell](https://github.com/noctalia-dev/noctalia)** (v5) | writes the theme as the custom palette `~/.config/noctalia/palettes/Grogu.json` and has the running shell reload it. Select it once in Noctalia's config: `[theme]` `source = "custom"`, `custom_palette = "Grogu"`. |
| **[niri](https://github.com/YaLTeR/niri)** | writes `~/.config/niri/grogu.kdl`, an include-able snippet with focus-ring colours. niri live-reloads. |
| **[teleia](https://github.com/foolish-dev/teleia)** | sets the `theme` row in teleia's sqlite prefs store (plus the exact extracted palette as `grogu_palette` in extract mode). teleia picks it up on next launch, or on SIGUSR1 with `--reload`. |
| **vim / neovim** | drops `~/.vim/colors/grogu.vim` and/or `~/.config/nvim/colors/grogu.vim`. Activate with `:colorscheme grogu`. |
| **[kitty](https://sw.kovidgoyal.net/kitty/)** | writes `~/.config/kitty/grogu.conf`. Activate by adding `include grogu.conf` to `kitty.conf` once; reload with `--reload`, `kill -SIGUSR1 $(pgrep kitty)`, or kitty's default `Ctrl+Shift+F5`. |
| **[ghostty](https://ghostty.org)** | writes `~/.config/ghostty/themes/grogu`. Activate by adding `theme = grogu` to `~/.config/ghostty/config`; ghostty live-reloads on save. |
| **[tmux](https://github.com/tmux/tmux)** | writes `~/.config/tmux/grogu.conf` (panes, messages, full status bar). Activate with `source-file ~/.config/tmux/grogu.conf` as the last line of `tmux.conf`; `--reload` re-sources it on a running server. |
| **SDDM greeter** | with `--extract`, copies the wallpaper into the noctalia SDDM theme's `Assets/background.png` and patches the `m*` colour keys in its `theme.conf`, so the login screen mirrors the desktop. Skips with a note when the theme isn't installed or files aren't writable. |
| **keyboard backlight** | `asusctl aura effect static -c <primary accent>` on ASUS Aura keyboards. Skips when `asusctl`/`asusd` is unavailable. |

Three themes ship: `tokyo-night`, `catppuccin`, `dracula`. Or extract
a palette directly from the current wallpaper (see "v2: wallpaper
extraction" below).

```sh
cargo install --path .

grogu list                       # tokyo-night / catppuccin / dracula
grogu paths                      # everywhere grogu reads or writes
grogu apply                      # defaults to teleia's stored theme pref
grogu apply --theme catppuccin
grogu apply --no-vim --dry-run   # see what would change without touching files
grogu apply --no-kitty --no-ghostty  # skip the terminals
grogu apply --reload             # also live-reload kitty, teleia and tmux

# v2: derive the whole palette from the current wallpaper
grogu apply --extract                              # asks the running Noctalia for its wallpaper
grogu apply --extract /path/to/wallpaper.jpg       # explicit path
grogu extract /path/to/wallpaper.jpg               # preview without writing
```

## v2: wallpaper extraction (pywal-style)

`grogu apply --extract` derives the full 16-colour palette from the
wallpaper itself instead of reading a predefined theme. The pipeline:

1. Load the image (PNG / JPEG / WebP / BMP via the `image` crate).
2. Resample to 256×256 for speed.
3. Convert to CIE Lab and run k-means clustering (k=12, three seeds,
   keep the lowest within-cluster variance).
4. Sort centroids by lightness; pin `bg` to the darkest cluster
   (clamped to L∈[6,16] so terminals stay readable on light wallpapers)
   and `fg` to the lightest (clamped to L∈[78,92]).
5. For each accent slot (red, yellow, green, cyan, blue, purple), pick
   the cluster closest to the canonical hue (25°, 80°, 130°, 180°,
   230°, 290°), then pull its chroma toward the target hue when the
   wallpaper doesn't have that colour — otherwise monochromatic
   wallpapers would collapse half the accents into one shade.
6. Derive the bright accents (ANSI 9–14) from the normal ones by
   raising Lab lightness, hue and chroma untouched. (Predefined themes
   use their upstream's canonical bright values instead.)

The extracted palette lands in every target:

- **Noctalia** — the same `palettes/Grogu.json` as predefined mode,
  filled with the extracted colours.
- **niri**, **kitty**, **ghostty**, **vim/neovim** — same renderers
  as predefined mode, just parameterised over the extracted palette.
- **teleia** — grogu writes the *nearest* predefined theme name
  (squared-distance on `bg + purple` in sRGB) as the fallback, plus the
  exact extracted palette as a `grogu_palette` JSON pref. teleia v0.2+
  reads the palette on SIGUSR1 (sent by `--reload`) and paints with the
  wallpaper-derived colours; older teleias just use the nearest theme.

Wallpaper source resolution order:

1. Explicit path: `grogu apply --extract /path/to/img.jpg`
2. `$GROGU_WALLPAPER` env var (intended for hook invocations)
3. The running Noctalia, asked with `noctalia msg wallpaper-get`.

Preview without writing:

```sh
grogu extract /path/to/img.jpg
```

prints the extracted palette so you can sanity-check what `apply
--extract` will do.

## Hook it to Noctalia's wallpaper rotation

Noctalia v5 runs hooks from its config. Add one that runs after a wallpaper
change; Noctalia hands it the new wallpaper in `$NOCTALIA_WALLPAPER_PATH`:

```toml
# ~/.config/noctalia/config.toml
[hooks]
wallpaper_changed = 'grogu apply --extract="$NOCTALIA_WALLPAPER_PATH" --reload'
```

(Drop `--extract` if you'd rather lock to a predefined theme;
`--reload` makes running kitty/teleia/tmux repaint immediately.)

Now every wallpaper rotation re-paints the rest of the desktop. niri
picks up the new colours automatically (live reload), teleia uses the
updated theme on next launch, and the next time you open vim,
`colorscheme grogu` reflects the new palette.

If you'd rather drive it from the compositor instead, bind it:

```hyprland
bind = SUPER SHIFT, T, exec, grogu apply
```

```kdl
# niri
binds {
    Mod+Shift+T { spawn "grogu" "apply"; }
}
```

## How the niri include works

niri's `include` directive merges `layout { ... }` properties across
files (per its docs) and appends `window-rule` blocks. So `grogu.kdl`
sets the focus-ring colours without clobbering the rest of your layout
config:

```kdl
# ~/.config/niri/config.kdl — add once
include "grogu.kdl"
```

Subsequent `grogu apply` runs rewrite `grogu.kdl` only; niri live-reloads.

## How the Noctalia palette works

Noctalia v5 loads custom palettes from `palettes/<name>.json` in its config
directory, so grogu writes every theme — predefined or extracted — to
`palettes/Grogu.json`: Material-style `m*` colours plus a `terminal`
block, the same for both modes. The write is atomic, so the shell never
reads a half file.

Then, if Noctalia is running, grogu runs `noctalia msg config-reload`, which
re-resolves the theme from disk, and sets the theme mode (`--light`, or dark)
with `noctalia msg theme-mode-set` only when it differs, since that persists
into Noctalia's GUI state. Without a running shell the palette simply waits
on disk for its next start.

grogu never selects one of Noctalia's built-in palettes: in v5 that would
take a GUI-state override, which then outranks your own config file.

## How the teleia integration works

grogu opens teleia's sqlite store at
`$XDG_DATA_HOME/teleia/teleia.sqlite` (or `~/.local/share/teleia/teleia.sqlite`)
and does an `INSERT OR REPLACE INTO prefs (key, value) VALUES ('theme', ?)`.
This is the same row teleia's `/theme NAME` slash command writes — teleia
reads it on every launch. In extract mode grogu also writes the full
palette to the `grogu_palette` pref (cleared again in predefined mode);
see the extraction section above.

If teleia hasn't run on the machine, grogu skips this target with a note.

## Terminal reload behaviour

- **ghostty** live-reloads its config on save, so a `grogu apply` repaints open windows automatically.
- **kitty**, **teleia** and **tmux** don't auto-detect changes. Pass
  `--reload` and grogu handles them itself: SIGUSR1 to every running
  `kitty` and `teleia` process, and `tmux source-file` against a running
  tmux server. Without `--reload`, grogu only writes files — reload
  manually with `kill -SIGUSR1 $(pgrep kitty)` / kitty's default
  `Ctrl+Shift+F5`, or wait for the next launch.

## Env overrides

- `NOCTALIA_CONFIG_DIR` — Noctalia's config directory, where `palettes/Grogu.json` goes (default `~/.config/noctalia`)
- `GROGU_WALLPAPER` — wallpaper to extract from (beats asking Noctalia, loses to an explicit `--extract PATH`)
- `GROGU_SDDM_GREETER_BG` / `GROGU_SDDM_GREETER_CONF` — the SDDM greeter files to sync
- `XDG_CONFIG_HOME` / `XDG_DATA_HOME` — standard XDG overrides for niri / teleia paths

## License

MIT.
