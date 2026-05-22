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
struct Theme {
    slug: &'static str,
    /// Noctalia's bundled scheme name, e.g. "Tokyo-Night".
    noctalia: &'static str,
    bg: &'static str,
    bg_hl: &'static str,
    fg: &'static str,
    dim: &'static str,
    red: &'static str,
    green: &'static str,
    yellow: &'static str,
    blue: &'static str,
    purple: &'static str,
    cyan: &'static str,
}

const THEMES: &[Theme] = &[
    Theme {
        slug: "tokyo-night",
        noctalia: "Tokyo-Night",
        bg: "#1a1b26",
        bg_hl: "#283457",
        fg: "#c0caf5",
        dim: "#414868",
        red: "#f7768e",
        green: "#9ece6a",
        yellow: "#e0af68",
        blue: "#7aa2f7",
        purple: "#bb9af7",
        cyan: "#7dcfff",
    },
    Theme {
        slug: "catppuccin",
        noctalia: "Catppuccin",
        bg: "#1e1e2e",
        bg_hl: "#313244",
        fg: "#cdd6f4",
        dim: "#45475a",
        red: "#f38ba8",
        green: "#a6e3a1",
        yellow: "#f9e2af",
        blue: "#89b4fa",
        purple: "#cba6f7",
        cyan: "#94e2d5",
    },
    Theme {
        slug: "dracula",
        noctalia: "Dracula",
        bg: "#282a36",
        bg_hl: "#44475a",
        fg: "#f8f8f2",
        dim: "#6272a4",
        red: "#ff5555",
        green: "#50fa7b",
        yellow: "#f1fa8c",
        blue: "#8be9fd",
        purple: "#bd93f9",
        cyan: "#8be9fd",
    },
];

fn find_theme(slug: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.slug == slug)
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
        #[arg(long, short)]
        theme: Option<String>,
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            for t in THEMES {
                println!("{:<14} -> noctalia:{}", t.slug, t.noctalia);
            }
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
        }
        Cmd::Apply {
            theme,
            no_noctalia,
            no_niri,
            no_telia,
            no_vim,
            light,
            dry_run,
        } => {
            let slug = match theme {
                Some(t) => t,
                None => read_telia_theme()?.unwrap_or_else(|| "tokyo-night".to_string()),
            };
            let theme = find_theme(&slug).ok_or_else(|| {
                anyhow!(
                    "unknown theme '{slug}' — known: {}",
                    THEMES.iter().map(|t| t.slug).collect::<Vec<_>>().join(", ")
                )
            })?;
            println!("theme: {}", theme.slug);
            if !no_noctalia {
                println!("  {}", apply_noctalia(theme, !light, dry_run)?);
            }
            if !no_niri {
                println!("  {}", apply_niri(theme, dry_run)?);
            }
            if !no_telia {
                println!("  {}", apply_telia(theme, dry_run)?);
            }
            if !no_vim {
                for line in apply_vim(theme, dry_run)? {
                    println!("  {line}");
                }
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
    if dry_run {
        return Ok(format!(
            "telia: would set prefs.theme = '{}' in {}",
            theme.slug,
            path.display()
        ));
    }
    let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
    // telia creates the prefs table on first launch; just trust it exists.
    conn.execute(
        "INSERT OR REPLACE INTO prefs (key, value) VALUES ('theme', ?1)",
        params![theme.slug],
    )?;
    Ok(format!(
        "telia: set prefs.theme = '{}' in {}",
        theme.slug,
        path.display()
    ))
}

// -------- noctalia: JSON-patch settings.json --------

fn apply_noctalia(theme: &Theme, dark: bool, dry_run: bool) -> Result<String> {
    let path = noctalia_settings_path()?;
    let mut doc: Value = if path.exists() {
        let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parse {} as JSON", path.display()))?
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
        Value::String(theme.noctalia.into()),
    );
    cs.insert("darkMode".into(), Value::Bool(dark));

    if dry_run {
        return Ok(format!(
            "noctalia: would set predefinedScheme={} darkMode={} at {}",
            theme.noctalia,
            dark,
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let pretty = serde_json::to_string_pretty(&doc)? + "\n";
    fs::write(&path, pretty).with_context(|| format!("write {}", path.display()))?;
    Ok(format!(
        "noctalia: set predefinedScheme={} darkMode={} in {}",
        theme.noctalia,
        dark,
        path.display()
    ))
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
