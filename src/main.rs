//! grogu — paint the desktop theme.
//!
//! Single binary. Given a theme slug (tokyo-night | catppuccin | dracula)
//! or `--from-telia` (reads telia's stored theme pref), grogu writes a
//! consistent palette across four targets:
//!
//! - **Noctalia shell** — patches `colorSchemes.predefinedScheme` in
//!   `~/.config/noctalia/settings.json` so its built-in scheme of the
//!   same name activates. Other keys are preserved verbatim.
//! - **niri** — writes `~/.config/niri/grogu.kdl`, an include-able KDL
//!   snippet with focus-ring colours. niri live-reloads on save.
//! - **telia** — writes the `theme` pref straight into telia's sqlite
//!   store. telia's `/theme` slash command picks it up on next launch.
//! - **vim / neovim** — emits a colorscheme to `~/.vim/colors/grogu.vim`
//!   and / or `~/.config/nvim/colors/grogu.vim`, whichever directory
//!   exists. Activate with `:colorscheme grogu`.
//!
//! Designed to run as a Noctalia post-wallpaper-change hook (see
//! README): when the wallpaper rotates, grogu repaints the system.

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// Palette for one theme. Hex strings, lowercase, with leading `#`.
/// Values lifted from the canonical theme definitions (Tokyo Night,
/// Catppuccin Mocha, Dracula) so the desktop reads identically to a
/// terminal running the same scheme.
#[derive(Clone)]
struct Theme {
    slug: String,
    /// Noctalia's bundled scheme name (e.g. "Tokyo-Night") OR "Grogu"
    /// when `noctalia_custom` is set — then we write a full custom
    /// scheme JSON to colorschemes/Grogu.json.
    noctalia: String,
    /// When true, write our own scheme JSON instead of relying on
    /// Noctalia's bundled scheme of the same name. Used by v2 extract
    /// mode where the palette comes from the wallpaper.
    noctalia_custom: bool,
    bg: String,
    bg_hl: String,
    fg: String,
    /// ANSI color8 / "bright black" / comments — softer than `black`.
    dim: String,
    /// ANSI color0 — the darkest neutral. Distinct from `bg` (a bit
    /// darker) and from `dim` (a bit darker still).
    black: String,
    /// ANSI color7 — "white" cell, slightly muted vs. `fg`.
    light_fg: String,
    red: String,
    green: String,
    yellow: String,
    blue: String,
    purple: String,
    cyan: String,
}

fn predefined_themes() -> Vec<Theme> {
    vec![
        Theme {
            slug: "tokyo-night".into(),
            noctalia: "Tokyo-Night".into(),
            noctalia_custom: false,
            bg: "#1a1b26".into(),
            bg_hl: "#283457".into(),
            fg: "#c0caf5".into(),
            dim: "#414868".into(),
            black: "#15161e".into(),
            light_fg: "#a9b1d6".into(),
            red: "#f7768e".into(),
            green: "#9ece6a".into(),
            yellow: "#e0af68".into(),
            blue: "#7aa2f7".into(),
            purple: "#bb9af7".into(),
            cyan: "#7dcfff".into(),
        },
        Theme {
            slug: "catppuccin".into(),
            noctalia: "Catppuccin".into(),
            noctalia_custom: false,
            bg: "#1e1e2e".into(),
            bg_hl: "#313244".into(),
            fg: "#cdd6f4".into(),
            dim: "#6c7086".into(),
            black: "#45475a".into(),
            light_fg: "#bac2de".into(),
            red: "#f38ba8".into(),
            green: "#a6e3a1".into(),
            yellow: "#f9e2af".into(),
            blue: "#89b4fa".into(),
            purple: "#cba6f7".into(),
            cyan: "#94e2d5".into(),
        },
        Theme {
            slug: "dracula".into(),
            noctalia: "Dracula".into(),
            noctalia_custom: false,
            bg: "#282a36".into(),
            bg_hl: "#44475a".into(),
            fg: "#f8f8f2".into(),
            dim: "#6272a4".into(),
            black: "#21222c".into(),
            light_fg: "#f8f8f2".into(),
            red: "#ff5555".into(),
            green: "#50fa7b".into(),
            yellow: "#f1fa8c".into(),
            blue: "#8be9fd".into(),
            purple: "#bd93f9".into(),
            cyan: "#8be9fd".into(),
        },
    ]
}

fn find_predefined(slug: &str) -> Option<Theme> {
    predefined_themes().into_iter().find(|t| t.slug == slug)
}

#[derive(Parser)]
#[command(
    name = "grogu",
    version,
    about = "Paint Noctalia + niri + telia + vim with one theme",
    long_about = "grogu propagates a Tokyo Night / Catppuccin / Dracula palette across the desktop. \
Designed to run as a Noctalia post-wallpaper-change hook so the system re-themes on every wallpaper rotation."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write theme files for every target.
    Apply {
        /// Theme slug (tokyo-night, catppuccin, dracula).
        /// Default: read from telia's prefs, or fall back to tokyo-night.
        /// Ignored when --extract is set.
        #[arg(long, short)]
        theme: Option<String>,
        /// Extract the palette from a wallpaper image instead of using
        /// a predefined theme. Pass a path explicitly, or omit the value
        /// to read Noctalia's current wallpaper from
        /// `~/.cache/noctalia/wallpapers.json`. Honours $GROGU_WALLPAPER.
        #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
        extract: Option<String>,
        /// Skip Noctalia.
        #[arg(long)]
        no_noctalia: bool,
        /// Skip niri.
        #[arg(long)]
        no_niri: bool,
        /// Skip telia.
        #[arg(long)]
        no_telia: bool,
        /// Skip vim / neovim.
        #[arg(long)]
        no_vim: bool,
        /// Skip kitty.
        #[arg(long)]
        no_kitty: bool,
        /// Skip ghostty.
        #[arg(long)]
        no_ghostty: bool,
        /// Use the light variant where supported (currently: Noctalia).
        #[arg(long)]
        light: bool,
        /// Print what would change without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// List known theme slugs.
    List,
    /// Show every path grogu reads or writes.
    Paths,
    /// Extract a palette from a wallpaper image and print it (no writes).
    /// Useful for previewing what `apply --extract` would produce.
    Extract {
        /// Wallpaper image path. Defaults to Noctalia's current wallpaper.
        path: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            for t in predefined_themes() {
                println!("{:<14} -> noctalia:{}", t.slug, t.noctalia);
            }
        }
        Cmd::Extract { path } => {
            let wallpaper = resolve_wallpaper(path.as_deref())?;
            let theme = extract_palette(&wallpaper)?;
            println!("wallpaper: {}", wallpaper.display());
            print_theme(&theme);
        }
        Cmd::Paths => {
            println!("telia sqlite      : {}", telia_db_path()?.display());
            println!(
                "noctalia settings : {}",
                noctalia_settings_path()?.display()
            );
            println!("niri snippet      : {}", niri_snippet_path()?.display());
            for p in vim_colorscheme_paths()? {
                println!("vim colorscheme   : {}", p.display());
            }
            println!("kitty conf        : {}", kitty_path()?.display());
            println!("ghostty theme     : {}", ghostty_path()?.display());
        }
        Cmd::Apply {
            theme,
            extract,
            no_noctalia,
            no_niri,
            no_telia,
            no_vim,
            no_kitty,
            no_ghostty,
            light,
            dry_run,
        } => {
            let theme = match extract {
                Some(p) => {
                    let path = if p.is_empty() { None } else { Some(p.as_str()) };
                    let wallpaper = resolve_wallpaper(path)?;
                    println!("extracting palette from: {}", wallpaper.display());
                    extract_palette(&wallpaper)?
                }
                None => {
                    let slug = match theme {
                        Some(t) => t,
                        None => read_telia_theme()?.unwrap_or_else(|| "tokyo-night".to_string()),
                    };
                    find_predefined(&slug).ok_or_else(|| {
                        anyhow!(
                            "unknown theme '{slug}' — known: {}",
                            predefined_themes()
                                .iter()
                                .map(|t| t.slug.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })?
                }
            };
            println!("theme: {}", theme.slug);
            if !no_noctalia {
                println!("  {}", apply_noctalia(&theme, !light, dry_run)?);
            }
            if !no_niri {
                println!("  {}", apply_niri(&theme, dry_run)?);
            }
            if !no_telia {
                println!("  {}", apply_telia(&theme, dry_run)?);
            }
            if !no_vim {
                for line in apply_vim(&theme, dry_run)? {
                    println!("  {line}");
                }
            }
            if !no_kitty {
                println!("  {}", apply_kitty(&theme, dry_run)?);
            }
            if !no_ghostty {
                println!("  {}", apply_ghostty(&theme, dry_run)?);
            }
            if dry_run {
                println!("(dry-run — no files written)");
            }
        }
    }
    Ok(())
}

// -------- path helpers --------

fn xdg_config() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(p));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".config"))
        .ok_or_else(|| anyhow!("neither XDG_CONFIG_HOME nor HOME is set"))
}

fn xdg_data() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(p));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/share"))
        .ok_or_else(|| anyhow!("neither XDG_DATA_HOME nor HOME is set"))
}

fn telia_db_path() -> Result<PathBuf> {
    Ok(xdg_data()?.join("telia/telia.sqlite"))
}

fn noctalia_settings_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("NOCTALIA_SETTINGS_FILE") {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("NOCTALIA_CONFIG_DIR") {
        return Ok(PathBuf::from(p).join("settings.json"));
    }
    Ok(xdg_config()?.join("noctalia/settings.json"))
}

fn niri_snippet_path() -> Result<PathBuf> {
    Ok(xdg_config()?.join("niri/grogu.kdl"))
}

fn kitty_path() -> Result<PathBuf> {
    Ok(xdg_config()?.join("kitty/grogu.conf"))
}

fn ghostty_path() -> Result<PathBuf> {
    Ok(xdg_config()?.join("ghostty/themes/grogu"))
}

/// Both classic vim and neovim colorscheme dirs. We write to whichever
/// already exists; if neither exists, we create the neovim one (most
/// common today) and skip vim.
fn vim_colorscheme_paths() -> Result<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(vec![
        home.join(".vim/colors/grogu.vim"),
        xdg_config()?.join("nvim/colors/grogu.vim"),
    ])
}

// -------- telia: read + write the theme pref --------

fn read_telia_theme() -> Result<Option<String>> {
    let path = telia_db_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    let mut stmt = conn.prepare("SELECT value FROM prefs WHERE key = 'theme'")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get::<_, String>(0)?))
    } else {
        Ok(None)
    }
}

fn apply_telia(theme: &Theme, dry_run: bool) -> Result<String> {
    let path = telia_db_path()?;
    if !path.exists() {
        return Ok(format!(
            "telia: skipped (no sqlite at {} — telia hasn't run here)",
            path.display()
        ));
    }
    // telia only ships three predefined themes — no custom-palette
    // support. For extracted palettes, pick the closest predefined
    // theme by squared-distance on accent colours.
    let telia_slug = if theme.noctalia_custom {
        nearest_predefined(theme)
    } else {
        theme.slug.clone()
    };
    if dry_run {
        return Ok(format!(
            "telia: would set prefs.theme = '{telia_slug}' in {}",
            path.display()
        ));
    }
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    conn.execute(
        "INSERT OR REPLACE INTO prefs (key, value) VALUES ('theme', ?1)",
        params![telia_slug],
    )?;
    Ok(format!(
        "telia: set prefs.theme = '{telia_slug}' in {}",
        path.display()
    ))
}

/// Pick the predefined theme whose `bg` + `purple` come closest to an
/// extracted palette's, by squared distance in sRGB. Good enough for
/// telia's three-option set — Tokyo Night, Catppuccin and Dracula are
/// distinct in accent hue, so the nearest match looks coherent.
fn nearest_predefined(theme: &Theme) -> String {
    let target_bg = hex_to_rgb_or_zero(&theme.bg);
    let target_purple = hex_to_rgb_or_zero(&theme.purple);
    predefined_themes()
        .into_iter()
        .min_by_key(|p| {
            let p_bg = hex_to_rgb_or_zero(&p.bg);
            let p_purple = hex_to_rgb_or_zero(&p.purple);
            sq_dist(p_bg, target_bg) + sq_dist(p_purple, target_purple)
        })
        .map(|t| t.slug)
        .unwrap_or_else(|| "tokyo-night".into())
}

fn hex_to_rgb_or_zero(hex: &str) -> [i32; 3] {
    let s = hex.trim_start_matches('#');
    if s.len() != 6 {
        return [0, 0, 0];
    }
    let parse = |a, b| i32::from_str_radix(&s[a..b], 16).unwrap_or(0);
    [parse(0, 2), parse(2, 4), parse(4, 6)]
}

fn sq_dist(a: [i32; 3], b: [i32; 3]) -> i64 {
    let dr = (a[0] - b[0]) as i64;
    let dg = (a[1] - b[1]) as i64;
    let db = (a[2] - b[2]) as i64;
    dr * dr + dg * dg + db * db
}

// -------- noctalia: JSON-patch settings.json --------

fn apply_noctalia(theme: &Theme, dark: bool, dry_run: bool) -> Result<String> {
    let settings_path = noctalia_settings_path()?;
    let mut doc: Value = if settings_path.exists() {
        let raw = fs::read_to_string(&settings_path)
            .with_context(|| format!("read {}", settings_path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parse {} as JSON", settings_path.display()))?
    } else {
        Value::Object(serde_json::Map::new())
    };
    let root = doc
        .as_object_mut()
        .ok_or_else(|| anyhow!("noctalia settings.json is not a JSON object"))?;
    let cs = root
        .entry("colorSchemes")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("colorSchemes is not a JSON object"))?;
    cs.insert("useWallpaperColors".into(), Value::Bool(false));
    cs.insert(
        "predefinedScheme".into(),
        Value::String(theme.noctalia.clone()),
    );
    cs.insert("darkMode".into(), Value::Bool(dark));

    // For extracted palettes we also write a full scheme JSON into
    // Noctalia's user colorschemes/ dir. Noctalia loads bundled + user
    // schemes by name, so "Grogu" becomes selectable next to the
    // built-ins.
    let custom_path = if theme.noctalia_custom {
        Some(noctalia_custom_scheme_path(&theme.noctalia)?)
    } else {
        None
    };

    if dry_run {
        let mut msg = format!(
            "noctalia: would set predefinedScheme={} darkMode={} at {}",
            theme.noctalia,
            dark,
            settings_path.display()
        );
        if let Some(p) = &custom_path {
            msg.push_str(&format!(
                "\n  noctalia: would write custom scheme JSON to {}",
                p.display()
            ));
        }
        return Ok(msg);
    }

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let pretty = serde_json::to_string_pretty(&doc)? + "\n";
    fs::write(&settings_path, pretty)
        .with_context(|| format!("write {}", settings_path.display()))?;

    let mut msg = format!(
        "noctalia: set predefinedScheme={} darkMode={} in {}",
        theme.noctalia,
        dark,
        settings_path.display()
    );
    if let Some(custom_path) = custom_path {
        if let Some(parent) = custom_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let scheme = noctalia_custom_scheme_json(theme);
        let pretty_scheme = serde_json::to_string_pretty(&scheme)? + "\n";
        fs::write(&custom_path, pretty_scheme)
            .with_context(|| format!("write {}", custom_path.display()))?;
        msg.push_str(&format!(
            "\n  noctalia: wrote custom scheme JSON to {}",
            custom_path.display()
        ));
    }
    Ok(msg)
}

fn noctalia_custom_scheme_path(name: &str) -> Result<PathBuf> {
    Ok(xdg_config()?
        .join("noctalia/colorschemes")
        .join(format!("{name}.json")))
}

/// Build a Noctalia-format scheme JSON from a Theme. Only the dark
/// variant is populated — Noctalia tolerates a partial document and
/// our extracted palettes don't have a sensible light variant.
fn noctalia_custom_scheme_json(t: &Theme) -> Value {
    serde_json::json!({
        "dark": {
            "mPrimary": t.blue,
            "mOnPrimary": t.bg,
            "mSecondary": t.purple,
            "mOnSecondary": t.bg,
            "mTertiary": t.green,
            "mOnTertiary": t.bg,
            "mError": t.red,
            "mOnError": t.bg,
            "mSurface": t.bg,
            "mOnSurface": t.fg,
            "mSurfaceVariant": t.bg_hl,
            "mOnSurfaceVariant": t.dim,
            "mOutline": t.dim,
            "mShadow": t.black,
            "mHover": t.green,
            "mOnHover": t.bg,
            "terminal": {
                "normal": {
                    "black": t.black,
                    "red": t.red,
                    "green": t.green,
                    "yellow": t.yellow,
                    "blue": t.blue,
                    "magenta": t.purple,
                    "cyan": t.cyan,
                    "white": t.light_fg,
                },
                "bright": {
                    "black": t.dim,
                    "red": t.red,
                    "green": t.green,
                    "yellow": t.yellow,
                    "blue": t.blue,
                    "magenta": t.purple,
                    "cyan": t.cyan,
                    "white": t.fg,
                },
                "foreground": t.fg,
                "background": t.bg,
                "selectionFg": t.fg,
                "selectionBg": t.bg_hl,
                "cursorText": t.bg,
                "cursor": t.fg,
            }
        }
    })
}

// -------- niri: include-able KDL snippet --------

fn apply_niri(theme: &Theme, dry_run: bool) -> Result<String> {
    let path = niri_snippet_path()?;
    let body = format!(
        r#"// Auto-generated by `grogu apply` — DO NOT EDIT.
// Source: grogu theme `{slug}`.
//
// Add this once to your ~/.config/niri/config.kdl (anywhere at top level):
//     include "grogu.kdl"
// niri live-reloads on save; re-run `grogu apply` to repaint.

layout {{
    focus-ring {{
        width 2
        active-color  "{blue}"
        inactive-color "{dim}"
    }}
}}

// Floating telia panel keyed by app-id; pair with foot's
// `--app-id=telia-float` launcher.
window-rule {{
    match app-id="telia-float"
    focus-ring {{
        active-color  "{purple}"
        inactive-color "{dim}"
        width 2
    }}
    border {{ off; }}
    geometry-corner-radius 12 12 12 12
}}
"#,
        slug = theme.slug,
        blue = theme.blue,
        dim = theme.dim,
        purple = theme.purple,
    );
    if dry_run {
        return Ok(format!(
            "niri: would write {} bytes to {}",
            body.len(),
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    fs::write(&path, &body).with_context(|| format!("write {}", path.display()))?;
    Ok(format!(
        "niri: wrote {} bytes to {}",
        body.len(),
        path.display()
    ))
}

// -------- vim: emit a colorscheme file --------

fn apply_vim(theme: &Theme, dry_run: bool) -> Result<Vec<String>> {
    let body = vim_colorscheme(theme);
    let mut reports = Vec::new();
    let targets = vim_colorscheme_paths()?;
    let mut wrote_any = false;
    for path in &targets {
        let parent_exists = path.parent().map(|p| p.parent()).is_some()
            && path
                .parent()
                .and_then(|p| p.parent())
                .map(|gp| gp.exists())
                .unwrap_or(false);
        // Only write if the editor is plausibly installed — i.e. the
        // grandparent dir (~/.vim or ~/.config/nvim) exists. This avoids
        // creating ~/.vim/colors/ on machines without vim.
        if !parent_exists {
            continue;
        }
        wrote_any = true;
        if dry_run {
            reports.push(format!(
                "vim: would write {} bytes to {}",
                body.len(),
                path.display()
            ));
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
        }
        fs::write(path, &body).with_context(|| format!("write {}", path.display()))?;
        reports.push(format!(
            "vim: wrote {} bytes to {}",
            body.len(),
            path.display()
        ));
    }
    if !wrote_any {
        // Default install spot: neovim. Create its colors/ dir and
        // write there. Users running classic vim can symlink or re-run
        // grogu after `mkdir -p ~/.vim/colors`.
        let nvim = targets
            .iter()
            .find(|p| p.to_string_lossy().contains("nvim"))
            .ok_or_else(|| anyhow!("vim colorscheme path enumeration is empty"))?;
        if dry_run {
            reports.push(format!(
                "vim: would write {} bytes to {} (neither vim nor nvim dir exists; defaulting to nvim)",
                body.len(),
                nvim.display()
            ));
        } else {
            if let Some(parent) = nvim.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("mkdir {}", parent.display()))?;
            }
            fs::write(nvim, &body).with_context(|| format!("write {}", nvim.display()))?;
            reports.push(format!(
                "vim: wrote {} bytes to {} (created nvim colors dir)",
                body.len(),
                nvim.display()
            ));
        }
    }
    Ok(reports)
}

fn vim_colorscheme(theme: &Theme) -> String {
    // Minimal but covers the syntax groups most files use. Vim falls
    // back to defaults for anything unset, so this stays readable.
    format!(
        r#"" grogu — generated by `grogu apply --theme {slug}`.
" Re-run grogu to refresh. To activate, add to your .vimrc / init.vim:
"     colorscheme grogu

hi clear
if exists("syntax_on") | syntax reset | endif
let g:colors_name = "grogu"
set background=dark

" --- editor chrome ---
hi Normal       guifg={fg}    guibg={bg}
hi NormalFloat  guifg={fg}    guibg={bg_hl}
hi LineNr       guifg={dim}   guibg=NONE
hi CursorLineNr guifg={yellow} guibg=NONE gui=bold
hi CursorLine                  guibg={bg_hl} cterm=NONE
hi CursorColumn                guibg={bg_hl}
hi Visual                      guibg={bg_hl}
hi VertSplit    guifg={dim}   guibg=NONE
hi StatusLine   guifg={fg}    guibg={bg_hl} gui=bold
hi StatusLineNC guifg={dim}   guibg={bg_hl}
hi Pmenu        guifg={fg}    guibg={bg_hl}
hi PmenuSel     guifg={bg}    guibg={purple} gui=bold
hi PmenuSbar                   guibg={bg_hl}
hi PmenuThumb                  guibg={purple}
hi MatchParen   guifg={cyan}  guibg=NONE gui=bold,underline
hi Search       guifg={bg}    guibg={yellow}
hi IncSearch    guifg={bg}    guibg={red}
hi Folded       guifg={dim}   guibg={bg_hl}
hi ColorColumn                 guibg={bg_hl}
hi SignColumn   guifg={dim}   guibg=NONE
hi NonText      guifg={dim}
hi SpecialKey   guifg={dim}
hi Directory    guifg={blue}
hi Title        guifg={purple} gui=bold

" --- syntax ---
hi Comment      guifg={dim}   gui=italic
hi Constant     guifg={yellow}
hi String       guifg={green}
hi Number       guifg={yellow}
hi Boolean      guifg={yellow}
hi Float        guifg={yellow}
hi Identifier   guifg={fg}
hi Function     guifg={blue}  gui=bold
hi Statement    guifg={purple}
hi Conditional  guifg={purple}
hi Repeat       guifg={purple}
hi Label        guifg={purple}
hi Operator     guifg={cyan}
hi Keyword      guifg={purple}
hi Exception    guifg={red}
hi PreProc      guifg={cyan}
hi Include      guifg={cyan}
hi Define       guifg={cyan}
hi Macro        guifg={cyan}
hi PreCondit    guifg={cyan}
hi Type         guifg={cyan}
hi StorageClass guifg={purple}
hi Structure    guifg={cyan}
hi Typedef      guifg={cyan}
hi Special      guifg={red}
hi SpecialChar  guifg={red}
hi Tag          guifg={red}
hi Delimiter    guifg={fg}
hi SpecialComment guifg={dim} gui=italic
hi Debug        guifg={red}
hi Underlined   guifg={blue}  gui=underline
hi Error        guifg={red}   guibg=NONE
hi Todo         guifg={yellow} guibg={bg_hl} gui=bold
hi DiffAdd      guifg={green} guibg={bg_hl}
hi DiffChange   guifg={yellow} guibg={bg_hl}
hi DiffDelete   guifg={red}   guibg={bg_hl}
hi DiffText     guifg={blue}  guibg={bg_hl} gui=bold

" --- diagnostics (nvim) ---
hi DiagnosticError guifg={red}
hi DiagnosticWarn  guifg={yellow}
hi DiagnosticInfo  guifg={blue}
hi DiagnosticHint  guifg={cyan}

" --- terminal colors (nvim) ---
let g:terminal_color_0  = "{bg}"
let g:terminal_color_1  = "{red}"
let g:terminal_color_2  = "{green}"
let g:terminal_color_3  = "{yellow}"
let g:terminal_color_4  = "{blue}"
let g:terminal_color_5  = "{purple}"
let g:terminal_color_6  = "{cyan}"
let g:terminal_color_7  = "{fg}"
let g:terminal_color_8  = "{dim}"
let g:terminal_color_9  = "{red}"
let g:terminal_color_10 = "{green}"
let g:terminal_color_11 = "{yellow}"
let g:terminal_color_12 = "{blue}"
let g:terminal_color_13 = "{purple}"
let g:terminal_color_14 = "{cyan}"
let g:terminal_color_15 = "{fg}"
"#,
        slug = theme.slug,
        bg = theme.bg,
        bg_hl = theme.bg_hl,
        fg = theme.fg,
        dim = theme.dim,
        red = theme.red,
        green = theme.green,
        yellow = theme.yellow,
        blue = theme.blue,
        purple = theme.purple,
        cyan = theme.cyan,
    )
}

// -------- kitty: theme.conf include --------

fn apply_kitty(theme: &Theme, dry_run: bool) -> Result<String> {
    let path = kitty_path()?;
    let body = kitty_conf(theme);
    if dry_run {
        return Ok(format!(
            "kitty: would write {} bytes to {}",
            body.len(),
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    fs::write(&path, &body).with_context(|| format!("write {}", path.display()))?;
    Ok(format!(
        "kitty: wrote {} bytes to {}",
        body.len(),
        path.display()
    ))
}

fn kitty_conf(theme: &Theme) -> String {
    // kitty's theme directives: key whitespace #rrggbb. Activate by
    // adding `include grogu.conf` to ~/.config/kitty/kitty.conf, then
    // SIGUSR1 to reload, or Ctrl+Shift+F5 (kitty's default reload key).
    format!(
        "# grogu — generated by `grogu apply --theme {slug}`.
# Activate in ~/.config/kitty/kitty.conf:
#     include grogu.conf
# Reload with `kill -SIGUSR1 $(pgrep kitty)` or restart kitty.

background            {bg}
foreground            {fg}
selection_background  {bg_hl}
selection_foreground  {fg}
cursor                {fg}
cursor_text_color     {bg}
url_color             {blue}

# normal
color0  {black}
color1  {red}
color2  {green}
color3  {yellow}
color4  {blue}
color5  {purple}
color6  {cyan}
color7  {light_fg}

# bright
color8  {dim}
color9  {red}
color10 {green}
color11 {yellow}
color12 {blue}
color13 {purple}
color14 {cyan}
color15 {fg}
",
        slug = theme.slug,
        bg = theme.bg,
        bg_hl = theme.bg_hl,
        fg = theme.fg,
        dim = theme.dim,
        black = theme.black,
        light_fg = theme.light_fg,
        red = theme.red,
        green = theme.green,
        yellow = theme.yellow,
        blue = theme.blue,
        purple = theme.purple,
        cyan = theme.cyan,
    )
}

// -------- ghostty: themes/grogu --------

fn apply_ghostty(theme: &Theme, dry_run: bool) -> Result<String> {
    let path = ghostty_path()?;
    let body = ghostty_conf(theme);
    if dry_run {
        return Ok(format!(
            "ghostty: would write {} bytes to {}",
            body.len(),
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    fs::write(&path, &body).with_context(|| format!("write {}", path.display()))?;
    Ok(format!(
        "ghostty: wrote {} bytes to {}",
        body.len(),
        path.display()
    ))
}

fn ghostty_conf(theme: &Theme) -> String {
    // ghostty's theme format: `key = value` per line, palette entries
    // use `palette = N=#rrggbb`. Activate by adding `theme = grogu` to
    // ~/.config/ghostty/config; ghostty live-reloads on save.
    format!(
        "# grogu — generated by `grogu apply --theme {slug}`.
# Activate in ~/.config/ghostty/config:
#     theme = grogu
# ghostty live-reloads on save.

background = {bg}
foreground = {fg}
selection-background = {bg_hl}
selection-foreground = {fg}
cursor-color = {fg}
cursor-text = {bg}

palette = 0={black}
palette = 1={red}
palette = 2={green}
palette = 3={yellow}
palette = 4={blue}
palette = 5={purple}
palette = 6={cyan}
palette = 7={light_fg}
palette = 8={dim}
palette = 9={red}
palette = 10={green}
palette = 11={yellow}
palette = 12={blue}
palette = 13={purple}
palette = 14={cyan}
palette = 15={fg}
",
        slug = theme.slug,
        bg = theme.bg,
        bg_hl = theme.bg_hl,
        fg = theme.fg,
        dim = theme.dim,
        black = theme.black,
        light_fg = theme.light_fg,
        red = theme.red,
        green = theme.green,
        yellow = theme.yellow,
        blue = theme.blue,
        purple = theme.purple,
        cyan = theme.cyan,
    )
}

// -------- v2: palette extraction from a wallpaper image --------

use kmeans_colors::{get_kmeans_hamerly, Kmeans, Sort};
use palette::{cast::from_component_slice, FromColor, IntoColor, Lab, Srgb};
use std::path::Path;

/// Decide which wallpaper to extract from. Precedence:
/// 1. Explicit `--extract PATH` argument
/// 2. `$GROGU_WALLPAPER` env var (intended for Noctalia hook invocation)
/// 3. Noctalia's wallpaper cache file
fn resolve_wallpaper(explicit: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        let pb = PathBuf::from(p);
        if !pb.exists() {
            return Err(anyhow!("wallpaper does not exist: {}", pb.display()));
        }
        return Ok(pb);
    }
    if let Some(p) = std::env::var_os("GROGU_WALLPAPER") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Ok(pb);
        }
    }
    let cache = noctalia_wallpaper_cache_path()?;
    if !cache.exists() {
        return Err(anyhow!(
            "no wallpaper specified and Noctalia cache not found at {}.\n\
             pass a path: `grogu apply --extract /path/to/wallpaper.jpg`",
            cache.display()
        ));
    }
    let raw = fs::read_to_string(&cache).with_context(|| format!("read {}", cache.display()))?;
    let json: Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {} as JSON", cache.display()))?;
    pick_noctalia_wallpaper(&json).ok_or_else(|| {
        anyhow!(
            "couldn't find an active wallpaper path in {} — set $GROGU_WALLPAPER or pass --extract PATH",
            cache.display()
        )
    })
}

fn noctalia_wallpaper_cache_path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("NOCTALIA_CACHE_DIR") {
        return Ok(PathBuf::from(p).join("wallpapers.json"));
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .ok_or_else(|| anyhow!("neither XDG_CACHE_HOME nor HOME is set"))?;
    Ok(base.join("noctalia/wallpapers.json"))
}

/// Walk Noctalia's wallpaper cache JSON looking for the first
/// readable wallpaper path. Schema has shifted across Noctalia
/// versions, so we scan recursively for any string that names an
/// existing file. We prefer the `dark` key when an object has one.
fn pick_noctalia_wallpaper(v: &Value) -> Option<PathBuf> {
    fn walk(v: &Value) -> Option<PathBuf> {
        match v {
            Value::String(s) => {
                let pb = PathBuf::from(s);
                if pb.is_file() {
                    return Some(pb);
                }
                None
            }
            Value::Array(arr) => arr.iter().find_map(walk),
            Value::Object(obj) => {
                if let Some(d) = obj.get("dark").and_then(walk) {
                    return Some(d);
                }
                obj.values().find_map(walk)
            }
            _ => None,
        }
    }
    walk(v)
}

const EXTRACT_K: usize = 12;
const EXTRACT_MAX_ITER: usize = 20;
const EXTRACT_SAMPLE_DIM: u32 = 256;

/// Extract a Theme from a wallpaper image via k-means clustering in
/// Lab colour space. Role assignment is heuristic; values are clamped
/// so terminals stay readable regardless of how dark/light the input is.
fn extract_palette(path: &Path) -> Result<Theme> {
    let img = image::open(path).with_context(|| format!("open {}", path.display()))?;
    let thumb = img
        .thumbnail(EXTRACT_SAMPLE_DIM, EXTRACT_SAMPLE_DIM)
        .to_rgb8();
    let rgb = thumb.into_raw();
    let srgb: &[Srgb<u8>] = from_component_slice(&rgb);
    let lab: Vec<Lab> = srgb
        .iter()
        .map(|p| p.into_format::<f32>().into_color())
        .collect();

    // Run k-means a few times with different seeds; keep the result
    // with the lowest "score" (within-cluster variance).
    let mut best: Option<Kmeans<Lab>> = None;
    for seed in [0u64, 1, 2] {
        let run = get_kmeans_hamerly(EXTRACT_K, EXTRACT_MAX_ITER, 5.0, false, &lab, seed);
        best = Some(match best.take() {
            Some(prev) if prev.score <= run.score => prev,
            _ => run,
        });
    }
    let run = best.ok_or_else(|| anyhow!("k-means returned no result"))?;
    let centroids: Vec<Lab> = Lab::sort_indexed_colors(&run.centroids, &run.indices)
        .into_iter()
        .map(|c| c.centroid)
        .collect();

    let mut by_l = centroids.clone();
    by_l.sort_by(|a, b| a.l.partial_cmp(&b.l).unwrap_or(std::cmp::Ordering::Equal));

    // Pin a usable bg/fg even if the wallpaper is uniformly light or
    // dark — terminals need contrast.
    let bg_lab = clamp_lightness(by_l[0], 6.0, 16.0);
    let bg_hl_lab = clamp_lightness(by_l.get(1).copied().unwrap_or(by_l[0]), 18.0, 28.0);
    let fg_lab = clamp_lightness(*by_l.last().unwrap(), 78.0, 92.0);
    let light_fg_lab = clamp_lightness(
        by_l.get(by_l.len().saturating_sub(2))
            .copied()
            .unwrap_or(fg_lab),
        65.0,
        82.0,
    );
    let dim_lab = by_l
        .get(by_l.len() / 4)
        .copied()
        .map(|c| clamp_lightness(c, 28.0, 45.0))
        .unwrap_or(bg_hl_lab);

    let mid: Vec<Lab> = by_l
        .iter()
        .filter(|c| c.l > 25.0 && c.l < 75.0)
        .copied()
        .collect();
    let accent_pool: Vec<Lab> = if mid.len() >= 3 {
        mid
    } else {
        centroids.clone()
    };

    let red = pick_accent(&accent_pool, 25.0);
    let yellow = pick_accent(&accent_pool, 80.0);
    let green = pick_accent(&accent_pool, 130.0);
    let cyan = pick_accent(&accent_pool, 180.0);
    let blue = pick_accent(&accent_pool, 230.0);
    let purple = pick_accent(&accent_pool, 290.0);

    let black_lab = Lab::new(bg_lab.l * 0.5, bg_lab.a, bg_lab.b);

    Ok(Theme {
        slug: "grogu-extracted".into(),
        noctalia: "Grogu".into(),
        noctalia_custom: true,
        bg: lab_to_hex(bg_lab),
        bg_hl: lab_to_hex(bg_hl_lab),
        fg: lab_to_hex(fg_lab),
        dim: lab_to_hex(dim_lab),
        black: lab_to_hex(black_lab),
        light_fg: lab_to_hex(light_fg_lab),
        red: lab_to_hex(red),
        green: lab_to_hex(green),
        yellow: lab_to_hex(yellow),
        blue: lab_to_hex(blue),
        purple: lab_to_hex(purple),
        cyan: lab_to_hex(cyan),
    })
}

fn clamp_lightness(c: Lab, min_l: f32, max_l: f32) -> Lab {
    let l = c.l.clamp(min_l, max_l);
    Lab::new(l, c.a, c.b)
}

/// Pick the cluster whose hue is closest to `target_hue_deg`, then
/// pull its chroma toward the target — strongly when the wallpaper
/// lacks that hue (otherwise red/yellow/green/cyan/blue/purple all
/// collapse to the same colour on monochromatic wallpapers).
fn pick_accent(pool: &[Lab], target_hue_deg: f32) -> Lab {
    let pick = pool
        .iter()
        .min_by(|a, b| {
            let da = hue_distance(lab_hue_deg(**a), target_hue_deg);
            let db = hue_distance(lab_hue_deg(**b), target_hue_deg);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .unwrap_or(Lab::new(60.0, 0.0, 0.0));
    let hue_dist = hue_distance(lab_hue_deg(pick), target_hue_deg);
    let chroma = (pick.a * pick.a + pick.b * pick.b).sqrt();
    // Aggressive blend when the wallpaper doesn't naturally have this
    // hue, mild blend when chroma is low, none when the match is good.
    let blend = if hue_dist > 45.0 {
        0.75
    } else if hue_dist > 20.0 {
        0.45
    } else if chroma < 25.0 {
        0.4
    } else {
        0.0
    };
    let target_rad = target_hue_deg.to_radians();
    let target_a = target_rad.cos() * 45.0;
    let target_b = target_rad.sin() * 45.0;
    let a = pick.a * (1.0 - blend) + target_a * blend;
    let b = pick.b * (1.0 - blend) + target_b * blend;
    let l = pick.l.clamp(55.0, 75.0);
    Lab::new(l, a, b)
}

fn lab_hue_deg(c: Lab) -> f32 {
    let h = c.b.atan2(c.a).to_degrees();
    if h < 0.0 {
        h + 360.0
    } else {
        h
    }
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

fn lab_to_hex(c: Lab) -> String {
    let rgb: Srgb = Srgb::from_color(c);
    let r = (rgb.red.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (rgb.green.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (rgb.blue.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn print_theme(t: &Theme) {
    println!("slug:     {}", t.slug);
    println!("noctalia: {} (custom: {})", t.noctalia, t.noctalia_custom);
    println!("bg       {}", t.bg);
    println!("bg_hl    {}", t.bg_hl);
    println!("fg       {}", t.fg);
    println!("light_fg {}", t.light_fg);
    println!("dim      {}", t.dim);
    println!("black    {}", t.black);
    println!("red      {}", t.red);
    println!("green    {}", t.green);
    println!("yellow   {}", t.yellow);
    println!("blue     {}", t.blue);
    println!("purple   {}", t.purple);
    println!("cyan     {}", t.cyan);
}
