//! Loads the Queen's Domain save folder (World*.sav + Challenges).

use crate::cursor::Res;
use crate::gvas;
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct WorldSave {
    pub file: PathBuf,
    pub last_save_ticks: i64,
    pub playtime_secs: i64,
    pub level: i64,
    pub suspend: bool,
    pub nsd: HashMap<String, i64>,
    pub inventory: HashMap<String, i64>,
    pub states: HashMap<String, i64>,
    pub ach_throwables: HashSet<String>,
    pub weapon_upgrades: HashMap<String, i64>,
    pub weapon_levels: HashMap<String, i64>,
    pub equipment: HashSet<String>,
}

impl WorldSave {
    pub fn nsd(&self, k: &str) -> i64 {
        self.nsd.get(k).copied().unwrap_or(0)
    }
    /// Current count in inventory (0 = item was evolved/merged/used up).
    pub fn count(&self, item: &str) -> i64 {
        self.inventory.get(item).copied().unwrap_or(0) + if self.equipment.contains(item) { 1 } else { 0 }
    }
    pub fn ever_had(&self, item: &str) -> bool {
        self.inventory.contains_key(item) || self.equipment.contains(item)
    }
    pub fn touched(&self, guids: &[String]) -> bool {
        guids.iter().any(|g| self.states.contains_key(g))
    }
}

#[derive(Default)]
pub struct ChallengeSave {
    pub stats: HashMap<String, i64>,
    pub completed: HashSet<String>,
}

pub struct SaveDir {
    pub dir: PathBuf,
    pub worlds: Vec<WorldSave>,
    pub challenges: ChallengeSave,
    pub errors: Vec<String>,
}

impl SaveDir {
    /// Most recently written world save (regular or suspend).
    pub fn latest(&self) -> Option<&WorldSave> {
        self.worlds.iter().max_by_key(|w| w.last_save_ticks)
    }
}

fn map_si(v: Option<&Value>) -> HashMap<String, i64> {
    v.map(|m| m.as_map().iter().filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_i64()?))).collect())
        .unwrap_or_default()
}

fn str_set(v: Option<&Value>) -> HashSet<String> {
    v.map(|a| a.as_array().iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn int(v: Option<&Value>) -> i64 {
    v.and_then(|x| x.as_i64()).unwrap_or(0)
}

fn parse_world(path: &Path) -> Res<WorldSave> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    let s = gvas::parse(&b)?;
    let r = &s.root;
    Ok(WorldSave {
        file: path.to_path_buf(),
        last_save_ticks: int(r.get("LastSaveTime")),
        playtime_secs: int(r.get("PlayTime_Seconds")),
        level: int(r.get("SavedPlayerLevel")),
        suspend: r.get("IsSuspendSave").and_then(|v| v.as_bool()).unwrap_or(false),
        nsd: map_si(r.get("NamedSaveData")),
        inventory: map_si(r.get("InventoryContents")),
        states: map_si(r.get("SavableActorStates")),
        ach_throwables: str_set(r.get("AchievementThrowables")),
        weapon_upgrades: map_si(r.get("WeaponUpgrades")),
        weapon_levels: map_si(r.get("WeaponLevels")),
        equipment: r
            .get("Equipment")
            .map(|m| m.as_map().iter().filter_map(|(k, _)| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    })
}

fn parse_challenges(path: &Path) -> Res<ChallengeSave> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    let s = gvas::parse(&b)?;
    Ok(ChallengeSave {
        stats: map_si(s.root.get("ChallengeStats")),
        completed: str_set(s.root.get("CompletedChallenges")),
    })
}

fn is_world(p: &Path) -> bool {
    let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    n.starts_with("world") && n.ends_with(".sav")
}

/// Accepts either the folder holding the .sav files or a parent folder
/// (e.g. the Steam userdata `remote` folder) containing one.
pub fn find_save_dir(dir: &Path) -> Res<PathBuf> {
    let has = |d: &Path| std::fs::read_dir(d).map(|it| it.flatten().any(|e| is_world(&e.path()))).unwrap_or(false);
    if has(dir) {
        return Ok(dir.to_path_buf());
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {}", dir.display(), e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subs.sort();
    for s in subs {
        if has(&s) {
            return Ok(s);
        }
    }
    Err(format!("no World*.sav files found in {}", dir.display()))
}

pub fn load(dir: &Path) -> Res<SaveDir> {
    let dir = find_save_dir(dir)?;
    let mut worlds = Vec::new();
    let mut errors = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    files.sort();
    for f in files.iter().filter(|f| is_world(f)) {
        match parse_world(f) {
            Ok(w) => worlds.push(w),
            Err(e) => errors.push(format!("{}: {}", f.display(), e)),
        }
    }
    let mut challenges = ChallengeSave::default();
    for name in ["Challenges", "Challenges_Backup"] {
        let p = dir.join(name);
        if p.exists() {
            match parse_challenges(&p) {
                Ok(c) => {
                    challenges = c;
                    break;
                }
                Err(e) => errors.push(format!("{}: {}", p.display(), e)),
            }
        }
    }
    if worlds.is_empty() {
        return Err(format!("could not read any world save in {} ({})", dir.display(), errors.join("; ")));
    }
    Ok(SaveDir { dir, worlds, challenges, errors })
}
