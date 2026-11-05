//! Extracts everything the report needs from the game's .pak:
//! achievement list, challenge definitions, display strings, item data,
//! shop stock and every save-tracked actor placed in the world maps.

use crate::cursor::Res;
use crate::pak::Pak;
use crate::value::Value;
use std::collections::{HashMap, HashSet};

pub struct ChallengeDef {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub mode: String,
    pub stat_max: i64,
    pub nsd: Vec<(String, i64)>,
    pub inventory: Vec<(String, i64)>,
}

pub struct Item {
    pub path: String,
    pub name: String,
    pub tags: Vec<String>,
    pub upgrade_index: Option<i64>,
    pub max_level: Option<i64>,
}

impl Item {
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map(|(a, _)| a).unwrap_or("")
    }
    pub fn has_tag(&self, t: &str) -> bool {
        self.tags.iter().any(|x| x == t)
    }
}

pub struct Actor {
    pub map: String,
    pub name: String,
    pub class: String,
    /// Package of the actor's class, e.g. `/Game/Items/Weapons/Daggers/Chakram/Chakram_Pickup`.
    pub class_pkg: String,
    pub guids: Vec<String>,
    /// World location (sublevel streaming offset applied).
    pub loc: Option<[f64; 3]>,
    pub no_count: bool,
    pub activatable: bool,
    pub activations: Option<i64>,
}

pub struct ShopEntry {
    pub price: i64,
    pub unlock_item: Option<String>,
    pub grants: Vec<String>,
}

pub struct GameData {
    pub achievements: Vec<String>,
    pub challenges: HashMap<String, ChallengeDef>,
    pub items: HashMap<String, Item>,
    pub actors: Vec<Actor>,
    pub shop: Vec<ShopEntry>,
    pub skull_rewards: Vec<(i64, String)>,
}

const SKIP_MAPS: &[&str] = &["ShaderCompile", "Test", "VideoConfig", "Journal3DView", "title_screen", "Entry"];

impl GameData {
    pub fn load(pak: &Pak) -> Res<GameData> {
        let strings = load_strings(pak).unwrap_or_else(|e| {
            eprintln!("warning: string table: {}", e);
            HashMap::new()
        });
        let tr = |s: &str| -> String {
            match s.strip_prefix('@') {
                Some(k) => strings.get(k).cloned().unwrap_or_else(|| k.to_string()),
                None => s.to_string(),
            }
        };

        let achievements = load_achievement_ids(pak)?;
        let challenges = load_challenges(pak, &tr)?;
        eprintln!("  {} achievements, {} challenge definitions", achievements.len(), challenges.len());
        let items = load_items(pak, &tr);
        eprintln!("  {} items", items.len());
        let shop = load_shop(pak).unwrap_or_else(|e| {
            eprintln!("warning: shop: {}", e);
            Vec::new()
        });
        let skull_rewards = load_skull_rewards(pak).unwrap_or_default();
        let actors = load_actors(pak);
        eprintln!("  {} save-tracked actors in world maps", actors.len());

        Ok(GameData { achievements, challenges, items, actors, shop, skull_rewards })
    }

    pub fn item_name(&self, path: &str) -> String {
        match self.items.get(path) {
            Some(i) if !i.name.is_empty() => i.name.clone(),
            _ => path.rsplit(['.', '/']).next().unwrap_or(path).to_string(),
        }
    }
}

fn load_achievement_ids(pak: &Pak) -> Res<Vec<String>> {
    let ini = pak.read(&format!("{}/Config/DefaultEngine.ini", pak.project))?;
    let text = String::from_utf8_lossy(&ini);
    let mut ids: Vec<(u32, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Achievement_") {
            if let Some((num, id)) = rest.split_once("_Id=") {
                if let Ok(n) = num.parse() {
                    ids.push((n, id.trim().to_string()));
                }
            }
        }
    }
    ids.sort();
    Ok(ids.into_iter().map(|(_, id)| id).collect())
}

fn load_strings(pak: &Pak) -> Res<HashMap<String, String>> {
    let a = pak.asset("/Game/StringTables/ImportedStringTable")?;
    let e = a.exports.first().ok_or("string table has no exports")?;
    let mut p = a.parser_at(e.off);
    p.props(Some(e.off + e.size))?;
    p.c.i32()?;
    p.c.fstring()?; // namespace
    let n = p.c.i32()?;
    let mut out = HashMap::new();
    for _ in 0..n.max(0) {
        let k = p.c.fstring()?;
        let v = p.c.fstring()?;
        out.insert(k, v);
    }
    Ok(out)
}

fn pairs_si(v: Option<&Value>) -> Vec<(String, i64)> {
    v.map(|m| {
        m.as_map()
            .iter()
            .filter_map(|(k, v)| {
                let key = match k {
                    Value::Struct(_) => k.get("TagName")?.as_str()?.to_string(),
                    _ => k.as_str()?.to_string(),
                };
                Some((key, v.as_i64()?))
            })
            .collect()
    })
    .unwrap_or_default()
}

fn load_challenges(pak: &Pak, tr: &dyn Fn(&str) -> String) -> Res<HashMap<String, ChallengeDef>> {
    let a = pak.asset("/Game/Blueprints/Challenges/FS_ChallengeData")?;
    let mut out = HashMap::new();
    for e in &a.exports {
        let p = a.export_props(e);
        let Some(defs) = p.get("ChallengeDefinitions") else { continue };
        for (k, d) in defs.as_map() {
            let Some(id) = k.as_str() else { continue };
            let s = |f: &str| d.get(f).and_then(|v| v.as_str()).unwrap_or("").to_string();
            out.insert(
                id.to_string(),
                ChallengeDef {
                    id: id.to_string(),
                    name: tr(&s("ImportedName")).trim().to_string(),
                    desc: tr(&s("ImportedDescription")),
                    mode: s("CompleteConditionMode").rsplit("::").next().unwrap_or("").to_string(),
                    stat_max: d.get("StatMax").and_then(|v| v.as_i64()).unwrap_or(0),
                    nsd: pairs_si(d.get("NSDCondition")),
                    inventory: pairs_si(d.get("InventoryCondition")),
                },
            );
        }
    }
    if out.is_empty() {
        return Err("no challenge definitions found".into());
    }
    Ok(out)
}

fn load_items(pak: &Pak, tr: &dyn Fn(&str) -> String) -> HashMap<String, Item> {
    let prefix = format!("{}/Content/", pak.project);
    let mut keys: Vec<&String> = pak
        .entries
        .keys()
        .filter(|k| k.starts_with(&prefix) && k.ends_with(".uasset") && (k.contains("/Items/") || k.contains("/Artifacts/")))
        .collect();
    keys.sort();
    let mut out = HashMap::new();
    for k in keys {
        let Ok(header) = pak.read(k) else { continue };
        if !contains(&header, b"FS_ItemData_C") {
            continue;
        }
        let Some(gp) = pak.game_path(k) else { continue };
        let Ok(a) = pak.asset(&gp) else { continue };
        for e in &a.exports {
            if a.class_of(e).0 != "FS_ItemData_C" {
                continue;
            }
            let p = a.export_props(e);
            let tags: Vec<String> = p
                .get("ItemTags")
                .map(|t| t.as_array().iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let mut item = Item {
                path: format!("{}.{}", gp, e.name),
                name: p.get("Name").and_then(|v| v.as_str()).map(tr).unwrap_or_default(),
                tags,
                upgrade_index: None,
                max_level: None,
            };
            if item.has_tag("ItemTags.AchievementWeapon") {
                if let Some(stats) = p.get("StatData").and_then(|v| v.as_str()) {
                    let sp = if stats.contains('.') {
                        pak.asset(stats).ok().and_then(|sa| {
                            let obj = stats.rsplit('.').next().unwrap_or("");
                            sa.exports.iter().find(|x| x.name == obj).map(|x| sa.export_props(x))
                        })
                    } else {
                        a.exports.iter().find(|x| x.name == stats).map(|x| a.export_props(x))
                    };
                    if let Some(sp) = sp {
                        item.upgrade_index = sp.get("UpgradeUnlockIndex").and_then(|v| v.as_i64());
                        item.max_level = sp.get("MaxWeaponLevel").and_then(|v| v.as_i64());
                    }
                }
            }
            out.insert(item.path.clone(), item);
        }
    }
    out
}

fn load_shop(pak: &Pak) -> Res<Vec<ShopEntry>> {
    let a = pak.asset("/Game/Blueprints/Shop/Shop")?;
    let mut out = Vec::new();
    for e in &a.exports {
        let p = a.export_props(e);
        let Some(stock) = p.get("Stock") else { continue };
        for (k, v) in stock.as_map() {
            if k.as_str().is_none() {
                continue;
            }
            let grants = v
                .get("ItemsToGrant")
                .map(|m| m.as_map().iter().filter_map(|(k, _)| k.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            out.push(ShopEntry {
                price: v.get("Price").and_then(|x| x.as_i64()).unwrap_or(0),
                unlock_item: v.get("UnlockItem").and_then(|x| x.as_str()).map(str::to_string),
                grants,
            });
        }
    }
    Ok(out)
}

fn load_skull_rewards(pak: &Pak) -> Res<Vec<(i64, String)>> {
    let a = pak.asset("/Game/NPCs/SecretSkullDoor/SecretSkullDoor_NPC_BP")?;
    let mut out = Vec::new();
    for e in a.exports.iter().filter(|e| e.name.starts_with("Default__")) {
        let p = a.export_props(e);
        for r in p.get("RewardList").map(|v| v.as_array()).unwrap_or(&[]) {
            let amount = r.get_prefix("AmountNeeded").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
            let reward = r
                .get_prefix("ItemPickup")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default(); // class object path of the reward pickup, empty = final ritual
            out.push((amount, reward));
        }
    }
    out.sort();
    Ok(out)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn load_actors(pak: &Pak) -> Vec<Actor> {
    let mut maps: Vec<String> = pak
        .entries
        .keys()
        .filter(|k| k.ends_with(".umap") && !SKIP_MAPS.iter().any(|s| k.contains(s)))
        .filter_map(|k| pak.game_path(k))
        .collect();
    maps.sort();

    let mut offsets: HashMap<String, [f64; 3]> = HashMap::new();
    let mut actors = Vec::new();
    // Per-instance "Don't count" overrides; None = use class default.
    let mut instance_flag: Vec<Option<bool>> = Vec::new();

    for m in &maps {
        let Ok(a) = pak.asset(m) else {
            eprintln!("warning: could not parse {}", m);
            continue;
        };
        let map_name = m.rsplit('/').next().unwrap_or(m).to_string();
        let props: Vec<Value> = a.exports.iter().map(|e| a.export_props(e)).collect();
        let mut children: HashMap<(i32, &str), usize> = HashMap::new();
        for (j, e) in a.exports.iter().enumerate() {
            children.insert((e.outer, e.name.as_str()), j);
        }
        for (i, e) in a.exports.iter().enumerate() {
            let p = &props[i];
            let (class, class_path) = a.class_of(e);
            if class.contains("LevelStreaming") {
                if let (Some(world), Some(t)) = (
                    p.get("WorldAsset").and_then(|v| v.as_str()),
                    p.get("LevelTransform").and_then(|t| t.get("Translation")).and_then(|v| v.as_vec3()),
                ) {
                    let short = world.rsplit('/').next().unwrap_or(world).split('.').next().unwrap_or("").to_string();
                    offsets.insert(short, t);
                }
            }
            let guids: Vec<String> = match p.get("GUIDs") {
                Some(g) => g.as_array().iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                None => continue,
            };
            if guids.is_empty() {
                continue;
            }
            let loc = p
                .get("RootComponent")
                .and_then(|v| v.as_str())
                .and_then(|root| children.get(&(i as i32 + 1, root)))
                .and_then(|&j| props[j].get("RelativeLocation"))
                .and_then(|v| v.as_vec3());
            instance_flag.push(p.get("Don'tCountForAchievement").and_then(|v| v.as_bool()));
            actors.push(Actor {
                map: map_name.clone(),
                name: e.name.clone(),
                class,
                class_pkg: class_path.split('.').next().unwrap_or("").to_string(),
                guids,
                loc,
                no_count: false,
                activatable: p.get("ActivatableMode").and_then(|v| v.as_bool()).unwrap_or(false),
                activations: p.get("ActivationCountRequirement").and_then(|v| v.as_i64()),
            });
        }
    }

    // Class defaults of the achievement flag (e.g. ScienceSecretWall sets it).
    let mut class_default: HashMap<String, bool> = HashMap::new();
    let classes: HashSet<String> = actors.iter().filter(|a| a.class.contains("SecretWall")).map(|a| a.class_pkg.clone()).collect();
    for pkg in classes {
        let mut flag = false;
        if let Ok(a) = pak.asset(&pkg) {
            for e in a.exports.iter().filter(|e| e.name.starts_with("Default__")) {
                if let Some(v) = a.export_props(e).get("Don'tCountForAchievement").and_then(|v| v.as_bool()) {
                    flag = v;
                }
            }
        }
        class_default.insert(pkg, flag);
    }

    for (a, inst) in actors.iter_mut().zip(instance_flag) {
        a.no_count = inst.unwrap_or_else(|| class_default.get(&a.class_pkg).copied().unwrap_or(false));
        if let (Some(l), Some(o)) = (a.loc.as_mut(), offsets.get(&a.map)) {
            for k in 0..3 {
                l[k] += o[k];
            }
        }
    }
    actors
}
