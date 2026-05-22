# grogu

A standalone Rust binary that paints a coherent palette across the whole
desktop in one shot:

| target | what it writes |
| --- | --- |
| **[Noctalia shell](https://github.com/noctalia-dev/noctalia-shell)** | patches `colorSchemes.predefinedScheme` + `darkMode` in `~/.config/noctalia/settings.json`. The matching built-in scheme activates immediately. |
| **[niri](https://github.com/YaLTeR/niri)** | writes `~/.config/niri/grogu.kdl`, an include-able snippet with focus-ring colours. niri live-reloads. |
| **[telia](https://github.com/foolish-dev/telia)** | sets the `theme` row in telia's sqlite prefs store. telia picks it up on next launch. |
| **vim / neovim** | drops `~/.vim/colors/grogu.vim` and/or `~/.config/nvim/colors/grogu.vim`. Activate with `:colorscheme grogu`. |
| **[kitty](https://sw.kovidgoyal.net/kitty/)** | writes `~/.config/kitty/grogu.conf`. Activate by adding `include grogu.conf` to `kitty.conf` once; reload with `kill -SIGUSR1 $(pgrep kitty)` or kitty's default `Ctrl+Shift+F5`. |
| **[ghostty](https://ghostty.org)** | writes `~/.config/ghostty/themes/grogu`. Activate by adding `theme = grogu` to `~/.config/ghostty/config`; ghostty live-reloads on save. |

Three themes ship: `tokyo-night`, `catppuccin`, `dracula`.

```sh
cargo install --path .

grogu list                       # tokyo-night / catppuccin / dracula
grogu paths                      # everywhere grogu reads or writes
grogu apply                      # defaults to telia's stored theme pref
grogu apply --theme catppuccin
grogu apply --no-vim --dry-run   # see what would change without touching files
grogu apply --no-kitty --no-ghostty  # skip the terminals
```

## Hook it to Noctalia's wallpaper rotation

Noctalia has a hooks system in its settings panel (Settings → Hooks).
Add a hook that runs after a wallpaper change:

```
event: wallpaper.changed
command: grogu apply
```

Now every wallpaper rotation re-paints the rest of the desktop. niri
picks up the new colours automatically (live reload), telia uses the
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

## How the Noctalia patch works

grogu reads the existing `settings.json` as JSON, mutates only
`colorSchemes.useWallpaperColors` / `colorSchemes.predefinedScheme` /
`colorSchemes.darkMode`, and writes the rest back verbatim. Other
top-level keys (`bar`, `dock`, custom keys you added) are preserved.

## How the telia integration works

grogu opens telia's sqlite store at
`$XDG_DATA_HOME/telia/telia.sqlite` (or `~/.local/share/telia/telia.sqlite`)
and does an `INSERT OR REPLACE INTO prefs (key, value) VALUES ('theme', ?)`.
This is the same row telia's `/theme NAME` slash command writes — telia
reads it on every launch.

If telia hasn't run on the machine, grogu skips this target with a note.

## Terminal reload behaviour

- **ghostty** live-reloads its config on save, so a `grogu apply` repaints open windows automatically.
- **kitty** doesn't auto-detect config changes; either set up `Ctrl+Shift+F5` (kitty's default reload bind), enable `allow_remote_control yes` and call `kitty @ set-colors --all --configured ~/.config/kitty/grogu.conf`, or just `kill -SIGUSR1 $(pgrep kitty)`.

If you want grogu to ping kitty after applying, run:

```sh
grogu apply && pkill -SIGUSR1 kitty 2>/dev/null || true
```

(grogu doesn't do this itself — it keeps the binary side-effect free
beyond file writes.)

## Env overrides

- `NOCTALIA_CONFIG_DIR` / `NOCTALIA_SETTINGS_FILE` — point at a non-default Noctalia install
- `XDG_CONFIG_HOME` / `XDG_DATA_HOME` — standard XDG overrides for niri / telia paths

## License

MIT.
