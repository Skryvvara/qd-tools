mod cursor;
mod game;
mod gvas;
mod pak;
mod report;
mod save;
mod uasset;
mod value;

use cursor::Res;
use std::path::{Path, PathBuf};

const USAGE: &str = "\
qd-save-report - Queen's Domain save game report

USAGE:
    qd-save-report <SAVE_DIR> [-o <OUTPUT.html>] [-g <GAME_DIR|PAK_FILE>]

ARGS:
    <SAVE_DIR>   Folder containing World0.sav / Challenges (or a parent folder)

OPTIONS:
    -o, --output <FILE>   Output HTML file (default: qd-report.html)
    -g, --game <PATH>     Game install folder or .pak file
                          (default: $QD_GAME_DIR or auto-detected Steam library)
    -h, --help            Show this help
";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Res<()> {
    let mut save_dir: Option<PathBuf> = None;
    let mut output = PathBuf::from("qd-report.html");
    let mut game: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{}", USAGE);
                return Ok(());
            }
            "-o" | "--output" => output = PathBuf::from(args.next().ok_or("missing value for --output")?),
            "-g" | "--game" => game = Some(PathBuf::from(args.next().ok_or("missing value for --game")?)),
            _ if a.starts_with('-') => return Err(format!("unknown option {}\n\n{}", a, USAGE)),
            _ => save_dir = Some(PathBuf::from(a)),
        }
    }
    let save_dir = save_dir.ok_or_else(|| USAGE.to_string())?;

    eprintln!("Reading saves from {}", save_dir.display());
    let sd = save::load(&save_dir)?;
    for e in &sd.errors {
        eprintln!("warning: {}", e);
    }

    let pak_path = find_pak(game.as_deref())?;
    eprintln!("Reading game data from {}", pak_path.display());
    let pak = pak::Pak::open(&pak_path)?;
    let gd = game::GameData::load(&pak)?;

    let html = report::render(&gd, &sd);
    std::fs::write(&output, html).map_err(|e| format!("{}: {}", output.display(), e))?;
    eprintln!("Wrote {}", output.display());
    Ok(())
}

fn pak_in(dir: &Path) -> Option<PathBuf> {
    for sub in ["QueensDomain/Content/Paks", "Content/Paks", "."] {
        let d = dir.join(sub);
        if let Ok(rd) = std::fs::read_dir(&d) {
            let mut paks: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == "pak").unwrap_or(false))
                .collect();
            paks.sort();
            if let Some(p) = paks.into_iter().next() {
                return Some(p);
            }
        }
    }
    None
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        roots.push(home.join("Library/Application Support/Steam"));
    }
    roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    roots.push(PathBuf::from(r"C:\Program Files\Steam"));
    roots
}

fn steam_libraries() -> Vec<PathBuf> {
    let mut libs = Vec::new();
    for root in steam_roots() {
        libs.push(root.clone());
        let vdf = root.join("steamapps/libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for line in text.lines() {
                let parts: Vec<&str> = line.split('"').collect();
                // line looks like:   "path"    "/mnt/storage/steam"
                if parts.len() >= 4 && parts[1] == "path" {
                    libs.push(PathBuf::from(parts[3].replace("\\\\", "\\")));
                }
            }
        }
    }
    libs
}

fn find_pak(game: Option<&Path>) -> Res<PathBuf> {
    if let Some(g) = game {
        if g.is_file() {
            return Ok(g.to_path_buf());
        }
        return pak_in(g).ok_or_else(|| format!("no .pak file found under {}", g.display()));
    }
    if let Some(g) = std::env::var_os("QD_GAME_DIR") {
        let g = PathBuf::from(g);
        return pak_in(&g).ok_or_else(|| format!("no .pak file found under $QD_GAME_DIR ({})", g.display()));
    }
    for lib in steam_libraries() {
        if let Some(p) = pak_in(&lib.join("steamapps/common/QueensDomain")) {
            return Ok(p);
        }
    }
    Err("could not find the Queen's Domain install; pass it with --game <path>".into())
}
