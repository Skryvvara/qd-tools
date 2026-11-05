//! Builds the self-contained HTML report.

use crate::game::{Actor, ChallengeDef, GameData, Item};
use crate::save::{SaveDir, WorldSave};
use std::collections::HashMap;
use std::fmt::Write;

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Friendly area name for a map file.
pub fn area(map: &str) -> &'static str {
    match map {
        "140_GR_ShopIsleOnly" => "Umbada's Islet",
        "760_Dekanima" => "Dekanima (off Record of Sin)",
        "760_SZ_AUT" => "Record of Sin",
        "740_SZ_Prison" => "Forgotten Dungeon",
        "770_SZ_Swamp" => "Rotting Cavern",
        "775_JigekiTemple" => "Jigeki's Temple",
        "790_SG" => "Petrified Garden",
        "800_FinalZone" => "Final Zone",
        "900_SipehaWorld" => "Sipeha's World",
        _ => match map.as_bytes().first() {
            Some(b'0') | Some(b'1') => "Isle of Neasied",
            Some(b'2') => "Obmuer Temple",
            Some(b'3') => "Tower of Knowledge",
            Some(b'4') => "Deimpo Mausoleum",
            Some(b'5') => "Ancient World",
            Some(b'6') => "Siraatsu Castle",
            Some(b'7') => "Underworld",
            _ => "",
        },
    }
}

/// UE FDateTime ticks (100ns since 0001-01-01) -> "YYYY-MM-DD HH:MM UTC".
fn ticks_to_string(ticks: i64) -> String {
    if ticks <= 0 {
        return "unknown".into();
    }
    let unix = ticks / 10_000_000 - 62_135_596_800;
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", y, m, d, secs / 3600, (secs / 60) % 60)
}

fn hours(secs: i64) -> String {
    format!("{}h {:02}m", secs / 3600, (secs / 60) % 60)
}

struct Ctx<'a> {
    gd: &'a GameData,
    sd: &'a SaveDir,
    w: &'a WorldSave,
    by_folder: HashMap<&'a str, Vec<&'a Item>>,
}

impl<'a> Ctx<'a> {
    fn uncollected(&self) -> impl Iterator<Item = &'a Actor> + '_ {
        self.gd.actors.iter().filter(move |a| !self.w.touched(&a.guids))
    }

    /// Counting secret walls split into (closed, switch-operated with unknown state).
    /// Walls opened by a switch/puzzle don't store their own state in the save
    /// (the switch reopens them on load), so they never show up as touched.
    /// If the game's own counter already covers every counting wall, nothing is closed.
    fn wall_status(&self) -> (Vec<&'a Actor>, Vec<&'a Actor>) {
        let total = self.gd.actors.iter().filter(|a| is_counting_wall(a)).count() as i64;
        if self.w.nsd("TotalSecretDoorsOpened") >= total {
            return (Vec::new(), Vec::new());
        }
        self.uncollected().filter(|a| is_counting_wall(a)).partition(|a| !a.activatable)
    }

    /// Best-matching item data for a pickup/actor class: same folder, and the
    /// item object name closest to the class name (`RitualSkull_2_Pickup1_C`
    /// -> `RitualSkull_2_Data1`).
    fn item_for_class(&self, class: &str, class_pkg: &str) -> Option<&'a Item> {
        let folder = class_pkg.rsplit_once('/').map(|(f, _)| f)?;
        let items = self.by_folder.get(folder)?;
        let base = class.split("_Pickup").next().unwrap_or(class).trim_end_matches("_C").trim_end_matches("_BP");
        let data = format!("{}_Data", base);
        items.iter().copied().filter(|i| !i.name.is_empty()).max_by_key(|i| {
            let obj = i.path.rsplit('.').next().unwrap_or("");
            let mut score = 0;
            if obj == base {
                score += 4;
            }
            if obj.starts_with(&data) {
                score += 3;
            } else if obj.starts_with(base) {
                score += 1;
            }
            if !i.tags.is_empty() {
                score += 2;
            }
            score
        })
    }

    fn pickup_item(&self, a: &Actor) -> Option<&'a Item> {
        self.item_for_class(&a.class, &a.class_pkg)
    }

    fn actor_label(&self, a: &Actor) -> String {
        match self.pickup_item(a) {
            Some(i) => i.name.clone(),
            None => a.class.trim_end_matches("_C").replace('_', " "),
        }
    }

    fn class_path_label(&self, path: &str) -> String {
        if path.is_empty() {
            return "The ritual".into();
        }
        let (pkg, class) = path.split_once('.').unwrap_or((path, path));
        match self.item_for_class(class, pkg) {
            Some(i) => i.name.clone(),
            None => class.trim_end_matches("_C").replace('_', " "),
        }
    }

    fn progress(&self, d: &ChallengeDef) -> (Option<i64>, Option<i64>) {
        let stat = self.sd.challenges.stats.get(&d.id).copied();
        let (cur, max) = match d.mode.as_str() {
            "Stat_AutoNSD" if d.id == "cha_All_Complete" => {
                let done = self.sd.challenges.completed.iter().filter(|c| *c != "cha_All_Complete").count() as i64;
                (Some(done), Some(d.stat_max))
            }
            "Stat_AutoNSD" => {
                let cur: i64 = d.nsd.iter().map(|(k, t)| self.w.nsd(k).min(*t)).sum();
                let tot: i64 = d.nsd.iter().map(|(_, t)| *t).sum();
                (Some(cur), Some(if d.stat_max > 0 { d.stat_max } else { tot }))
            }
            "Stat_AutoInventory" => {
                let cur: i64 = d.inventory.iter().map(|(k, t)| self.w.count(k).min(*t)).sum();
                let tot: i64 = d.inventory.iter().map(|(_, t)| *t).sum();
                (Some(cur), Some(tot))
            }
            "Manual" => (None, None),
            _ => (stat, if d.stat_max > 0 { Some(d.stat_max) } else { None }),
        };
        let cur = match (cur, stat) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        (cur, max)
    }
}

fn loc_cells(a: &Actor) -> String {
    match a.loc {
        Some([x, y, z]) => format!(
            "<td class=num>{:.0}</td><td class=num>{:.0}</td><td class=num>{:.0}</td>",
            x, y, z
        ),
        None => "<td colspan=3 class=muted>unknown</td>".into(),
    }
}

fn place_cells(a: &Actor) -> String {
    format!("<td>{}</td><td class=mono>{}</td>", esc(area(&a.map)), esc(&a.map))
}

const LOC_HEAD: &str = "<th>Area</th><th>Map</th><th class=num>X</th><th class=num>Y</th><th class=num>Z</th>";

fn table(head: &str, rows: &[String]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "<div class=tablewrap><table><thead><tr>{}</tr></thead><tbody>{}</tbody></table></div>",
        head,
        rows.join("")
    )
}

fn actor_rows(ctx: &Ctx, actors: &[&Actor], note: &dyn Fn(&Actor) -> String) -> Vec<String> {
    let mut v: Vec<&&Actor> = actors.iter().collect();
    v.sort_by(|a, b| a.map.cmp(&b.map).then(a.name.cmp(&b.name)));
    v.iter()
        .map(|a| {
            format!(
                "<tr><td>{}</td>{}{}<td class=muted>{}</td></tr>",
                esc(&ctx.actor_label(a)),
                place_cells(a),
                loc_cells(a),
                note(a)
            )
        })
        .collect()
}

fn actor_table(ctx: &Ctx, actors: &[&Actor], note: &dyn Fn(&Actor) -> String) -> String {
    let head = format!("<th>Object</th>{}<th>Notes</th>", LOC_HEAD);
    table(&head, &actor_rows(ctx, actors, note))
}

fn no_note(_: &Actor) -> String {
    String::new()
}

fn wall_note(a: &Actor) -> String {
    let mut s = if a.activatable { "Opened by a switch / puzzle".to_string() } else { "Interact to open".to_string() };
    if let Some(n) = a.activations.filter(|n| *n > 1) {
        let _ = write!(s, " ({} activations)", n);
    }
    s
}

fn is_counting_wall(a: &Actor) -> bool {
    a.class.contains("SecretWall") && !a.no_count
}

fn is_chest(a: &Actor) -> bool {
    a.class.contains("Chest") && !a.class.contains("Pickup") && !a.class.contains("Runecharm")
}

fn category(a: &Actor) -> Option<&'static str> {
    let p = a.class_pkg.as_str();
    if a.class.starts_with("SkullKeyRelic") {
        Some("Skull Keys")
    } else if a.class.contains("SecretWall") {
        if a.no_count { None } else { Some("Secret walls (count for the achievement)") }
    } else if is_chest(a) {
        Some("Chests")
    } else if a.class.contains("FishWife_Egg") {
        Some("Orbewa eggs")
    } else if p.starts_with("/Game/Items/Weapons/Daggers/") {
        Some("Throwing weapons")
    } else if p.starts_with("/Game/Items/Weapons/") {
        Some("Main weapons")
    } else if p.starts_with("/Game/Items/Treasure/") && !p.contains("/Coins/") {
        Some("Treasures")
    } else if p.starts_with("/Game/Items/Accessories/") {
        Some("Accessories")
    } else if p.starts_with("/Game/Items/Upgrades/") || p.starts_with("/Game/Items/KeyItems/") || p.starts_with("/Game/Artifacts/") {
        Some("Upgrades & key items")
    } else if p.contains("GrimoireInk") {
        Some("Grimoire ink")
    } else {
        None
    }
}

fn details(ctx: &Ctx, d: &ChallengeDef) -> String {
    let w = ctx.w;
    let gd = ctx.gd;
    let mut h = String::new();

    // Generic condition breakdown.
    if d.nsd.len() > 1 || (!d.inventory.is_empty() && d.mode == "Stat_AutoInventory") {
        let mut rows = Vec::new();
        for (k, t) in &d.nsd {
            let have = w.nsd(k);
            rows.push(format!(
                "<tr class=\"{}\"><td class=mono>{}</td><td class=num>{}</td><td class=num>{}</td></tr>",
                if have >= *t { "ok" } else { "miss" },
                esc(k),
                have,
                t
            ));
        }
        if d.mode == "Stat_AutoInventory" {
            for (k, t) in &d.inventory {
                let have = w.count(k);
                rows.push(format!(
                    "<tr class=\"{}\"><td>{}</td><td class=num>{}</td><td class=num>{}</td></tr>",
                    if have >= *t { "ok" } else { "miss" },
                    esc(&gd.item_name(k)),
                    have,
                    t
                ));
            }
        }
        h += &table("<th>Condition</th><th class=num>Have</th><th class=num>Need</th>", &rows);
    }

    match d.id.as_str() {
        "cha_Item_GotAllThrowables" => {
            let mut missing: Vec<&Item> = gd
                .items
                .values()
                .filter(|i| i.has_tag("ItemTags.AchievementThrowables") && !w.ach_throwables.contains(&i.path))
                .collect();
            missing.sort_by(|a, b| a.name.cmp(&b.name));
            for it in missing {
                let _ = write!(h, "<h4>{}</h4>", esc(&it.name));
                let pickups: Vec<&Actor> = ctx.uncollected().filter(|a| ctx.pickup_item(a).map(|x| x.path == it.path).unwrap_or(false)).collect();
                for s in gd.shop.iter().filter(|s| s.grants.contains(&it.path)) {
                    let gated = s.unlock_item.as_deref() == Some(it.path.as_str());
                    let _ = write!(
                        h,
                        "<p class=hint>Sold at Umbada's shop for {} coins{}.</p>",
                        s.price,
                        if gated { ", but only listed once you have obtained one yourself" } else { "" }
                    );
                }
                if pickups.is_empty() {
                    h += "<p class=hint>No uncollected world pickup found for this item in the game data.</p>";
                } else {
                    h += "<p class=hint>Uncollected world pickup(s) (several entries at the same spot are alternative variants):</p>";
                    h += &actor_table(ctx, &pickups, &no_note);
                }
            }
        }
        "cha_Item_GotAllWeps" | "cha_Item_GotAllAcc" => {
            let tag = if d.id == "cha_Item_GotAllWeps" { "ItemTags.AchievementWeapon" } else { "ItemTags.AchievementAccessory" };
            let mut missing: Vec<&Item> = gd.items.values().filter(|i| i.has_tag(tag) && !w.ever_had(&i.path)).collect();
            missing.sort_by(|a, b| a.name.cmp(&b.name));
            let rows: Vec<String> = missing.iter().map(|i| format!("<tr><td>{}</td></tr>", esc(&i.name))).collect();
            if !rows.is_empty() {
                h += "<p class=hint>Never obtained in this save:</p>";
                h += &table("<th>Item</th>", &rows);
            }
        }
        "cha_Item_UpgradeAllWeps" => {
            let mut todo: Vec<&Item> = gd
                .items
                .values()
                .filter(|i| i.upgrade_index.is_some() && !w.weapon_upgrades.contains_key(&i.path))
                .collect();
            todo.sort_by(|a, b| a.upgrade_index.cmp(&b.upgrade_index).then(a.name.cmp(&b.name)));
            let need = (d.stat_max - w.weapon_upgrades.len() as i64).max(0);
            let _ = write!(
                h,
                "<p class=hint>Upgrade {} more of these {} weapons at Ossi's upgrade machine. A weapon must be fully levelled and its upgrade data installed.</p>",
                need,
                todo.len()
            );
            let rows: Vec<String> = todo
                .iter()
                .map(|i| {
                    let idx = i.upgrade_index.unwrap_or(0);
                    let installed = w.nsd(&format!("WeaponUpgradeUnlock_{:02}", idx)) > 0;
                    let lv = w.weapon_levels.get(&i.path).map(|l| l.to_string()).unwrap_or_else(|| "0".into());
                    let max = i.max_level.map(|m| m - 1).unwrap_or(9);
                    let owned = w.count(&i.path) > 0;
                    format!(
                        "<tr><td>{}</td><td class=num>{} / {}</td><td class=num>#{} {}</td><td>{}</td></tr>",
                        esc(&i.name),
                        lv,
                        max,
                        idx,
                        if installed { "<span class=ok>installed</span>" } else { "<span class=miss>missing</span>" },
                        if owned { "yes" } else { "<span class=miss>no</span>" }
                    )
                })
                .collect();
            h += &table("<th>Weapon</th><th class=num>Level</th><th class=num>Upgrade data</th><th>In inventory</th>", &rows);
        }
        "cha_Challenge_AllSecretDoors" => {
            let total = gd.actors.iter().filter(|a| is_counting_wall(a)).count() as i64;
            let opened = w.nsd("TotalSecretDoorsOpened");
            let target = d.nsd.iter().find(|(k, _)| k == "TotalSecretDoorsOpened").map(|(_, t)| *t).unwrap_or(d.stat_max);
            let (closed, unknown) = ctx.wall_status();
            let _ = write!(
                h,
                "<p class=hint>Game counter <code>TotalSecretDoorsOpened</code>: <b>{}</b> of {} needed. The game data has <b>{}</b> secret walls that count.</p>",
                opened, target, total
            );
            if opened >= total && target > total {
                let _ = write!(
                    h,
                    "<p class=hint><b>Every counting secret wall is open.</b> The achievement asks for {} but only {} walls in the game increment the counter, so it can't be reached by opening walls. This looks like a bug in the game, not something you missed.</p>",
                    target, total
                );
            }
            if !closed.is_empty() {
                let _ = write!(h, "<p class=hint>{} closed:</p>", closed.len());
                h += &actor_table(ctx, &closed, &wall_note);
            }
            if !unknown.is_empty() {
                let _ = write!(h, "<p class=hint>{} switch-operated walls. The save doesn't record whether these are open; check them in game:</p>", unknown.len());
                h += &actor_table(ctx, &unknown, &wall_note);
            }
        }
        "cha_Challenge_AllChests" => {
            let chests: Vec<&Actor> = ctx.uncollected().filter(|a| is_chest(a)).collect();
            h += &actor_table(ctx, &chests, &no_note);
        }
        "cha_Quest_Fish" => {
            let eggs: Vec<&Actor> = ctx.uncollected().filter(|a| a.class.contains("FishWife_Egg")).collect();
            h += &actor_table(ctx, &eggs, &no_note);
        }
        "cha_Item_GotAllTreasures" => {
            let t: Vec<&Actor> = ctx.uncollected().filter(|a| category(a) == Some("Treasures")).collect();
            h += &actor_table(ctx, &t, &no_note);
        }
        "cha_Quest_Skulls" => {
            let turned = w.nsd("SkullsTurnedIn");
            let held = gd
                .items
                .keys()
                .find(|k| k.ends_with("SkullKeyRelic_Data.SkullKeyRelic_Data"))
                .map(|k| w.count(k))
                .unwrap_or(0);
            let _ = write!(
                h,
                "<p class=hint>Skull Keys turned in at the Secret Skull Door: <b>{}</b>, carried: <b>{}</b>.</p>",
                turned, held
            );
            let rows: Vec<String> = gd
                .skull_rewards
                .iter()
                .map(|(n, r)| {
                    format!(
                        "<tr class=\"{}\"><td class=num>{}</td><td>{}</td></tr>",
                        if turned >= *n { "ok" } else { "miss" },
                        n,
                        esc(&ctx.class_path_label(r))
                    )
                })
                .collect();
            h += &table("<th class=num>Skulls</th><th>Reward</th>", &rows);
            let skulls: Vec<&Actor> = ctx.uncollected().filter(|a| a.class.starts_with("SkullKeyRelic")).collect();
            if !skulls.is_empty() {
                h += "<p class=hint>Skull Keys still in the world:</p>";
                h += &actor_table(ctx, &skulls, &no_note);
            }
        }
        "cha_Endings_4" => {
            if w.nsd("SkullRitualCompleted") == 0 {
                h += "<p class=hint>Sipeha only offers the \u{201c}Eat\u{201d} choice once the skull ritual is complete (flag <span class=mono>SkullRitualCompleted</span>). Finish <i>The Call From Beyond</i> first.</p>";
            }
        }
        "cha_Item_FlyingSword" => {
            for (k, _) in &d.inventory {
                if w.count(k) == 0 && w.ever_had(k) {
                    h += "<p class=hint>You picked the sword up, but it has since been evolved, so this inventory check can no longer pass in this save. Picking it up in a fresh save slot should unlock it (achievements are account-wide).</p>";
                }
            }
        }
        "cha_All_Complete" => {
            h += "<p class=hint>Unlocks automatically once every other achievement is done.</p>";
        }
        _ => {}
    }
    h
}

fn bar(cur: Option<i64>, max: Option<i64>) -> String {
    match (cur, max) {
        (Some(c), Some(m)) if m > 0 => {
            let pct = ((c as f64 / m as f64) * 100.0).clamp(0.0, 100.0);
            format!(
                "<div class=progress><div class=track><div class=fill style=\"width:{:.1}%\"></div></div><span class=num>{} / {}</span></div>",
                pct, c, m
            )
        }
        _ => String::new(),
    }
}

const CSS: &str = r#"
:root{--bg:#f6f5f1;--card:#ffffff;--ink:#1d1c1a;--muted:#6b675f;--line:#e3e0d8;--accent:#7a4fd6;--ok:#2f7d4f;--miss:#b4402a;--track:#ebe8e0;color-scheme:light}
@media (prefers-color-scheme:dark){:root{--bg:#141317;--card:#1d1c22;--ink:#ecebf0;--muted:#9b98a6;--line:#2e2c35;--accent:#a88bff;--ok:#5cc286;--miss:#ff7a61;--track:#2b2933;color-scheme:dark}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:1100px;margin:0 auto;padding:32px 16px 64px}
h1{font-size:28px;margin:0 0 4px;letter-spacing:-.01em}
h2{font-size:19px;margin:40px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--line)}
h3{font-size:16px;margin:0}
h4{font-size:14px;margin:16px 0 4px}
.meta{color:var(--muted);margin:0 0 20px}
.tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:12px}
.tile{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 16px}
.tile .v{font-size:24px;font-weight:650;font-variant-numeric:tabular-nums}
.tile .l{color:var(--muted);font-size:13px}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px 18px;margin:12px 0}
.card .desc{color:var(--muted);margin:2px 0 8px}
.id{font:12px ui-monospace,SFMono-Regular,Menlo,monospace;color:var(--muted);margin-left:8px}
.progress{display:flex;align-items:center;gap:10px;margin:6px 0 4px}
.track{flex:1;max-width:340px;height:8px;background:var(--track);border-radius:99px;overflow:hidden}
.fill{height:100%;background:var(--accent);border-radius:99px}
.hint{color:var(--muted);margin:8px 0}
.tablewrap{overflow-x:auto;margin:8px 0}
table{border-collapse:collapse;width:100%;font-size:14px}
th,td{text-align:left;padding:5px 10px;border-bottom:1px solid var(--line);white-space:nowrap}
th{font-weight:600;color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.04em}
.num{text-align:right;font-variant-numeric:tabular-nums}
.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:13px}
.muted{color:var(--muted)}
tr.ok td:first-child,.ok{color:var(--ok)}
tr.miss td:first-child,.miss{color:var(--miss)}
details{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 16px;margin:10px 0}
summary{cursor:pointer;font-weight:600}
summary .count{color:var(--muted);font-weight:400;margin-left:6px}
.done{display:grid;grid-template-columns:repeat(auto-fill,minmax(230px,1fr));gap:6px 16px;margin:10px 0 4px;padding:0;list-style:none}
.done li{padding:2px 0}.done li::before{content:"\2713  ";color:var(--ok)}
footer{margin-top:48px;color:var(--muted);font-size:13px}
"#;

pub fn render(gd: &GameData, sd: &SaveDir) -> String {
    let w = sd.latest().expect("at least one world save");
    let mut by_folder: HashMap<&str, Vec<&Item>> = HashMap::new();
    for it in gd.items.values() {
        by_folder.entry(it.folder()).or_default().push(it);
    }
    let ctx = Ctx { gd, sd, w, by_folder };

    let total = gd.achievements.len();
    let done_ids: Vec<&String> = gd.achievements.iter().filter(|a| sd.challenges.completed.contains(*a)).collect();
    let missing_ids: Vec<&String> = gd.achievements.iter().filter(|a| !sd.challenges.completed.contains(*a)).collect();

    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Queen's Domain save report</title><style>{}</style></head><body><main>",
        CSS
    );
    let _ = write!(
        h,
        "<h1>Queen's Domain \u{2014} save report</h1><p class=meta>Based on <span class=mono>{}</span>{} \u{00b7} saved {} \u{00b7} folder <span class=mono>{}</span></p>",
        esc(&w.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        if w.suspend { " (suspend save)" } else { "" },
        ticks_to_string(w.last_save_ticks),
        esc(&sd.dir.display().to_string())
    );

    let (closed_walls, unknown_walls) = ctx.wall_status();
    let walls = closed_walls.len() + unknown_walls.len();
    let skulls = ctx.uncollected().filter(|a| a.class.starts_with("SkullKeyRelic")).count();
    let _ = write!(
        h,
        "<div class=tiles><div class=tile><div class=v>{} / {}</div><div class=l>Achievements</div></div><div class=tile><div class=v>{}</div><div class=l>Play time</div></div><div class=tile><div class=v>{}</div><div class=l>Player level</div></div><div class=tile><div class=v>{}</div><div class=l>Skull Keys left in world</div></div><div class=tile><div class=v>{}</div><div class=l>Counting secret walls not open</div></div></div>",
        done_ids.len(),
        total,
        hours(w.playtime_secs),
        w.level,
        skulls,
        walls
    );

    // Missing achievements
    let _ = write!(h, "<h2>Still to do ({})</h2>", missing_ids.len());
    if missing_ids.is_empty() {
        h += "<p>Every achievement is complete.</p>";
    }
    for id in &missing_ids {
        match gd.challenges.get(id.as_str()) {
            Some(d) => {
                let (cur, max) = ctx.progress(d);
                let _ = write!(
                    h,
                    "<section class=card><h3>{}<span class=id>{}</span></h3><p class=desc>{}</p>{}{}</section>",
                    esc(&d.name),
                    esc(id),
                    esc(&d.desc),
                    bar(cur, max),
                    details(&ctx, d)
                );
            }
            None => {
                let _ = write!(h, "<section class=card><h3>{}</h3><p class=desc>No challenge definition found.</p></section>", esc(id));
            }
        }
    }

    // World collectibles
    h += "<h2>Uncollected in the world</h2><p class=hint>Coordinates are Unreal world units (X / Y / Z), the same values the CoordsDisplay mod shows in game. Some pickups exist in several variants at the same spot; collecting one is enough.</p>";
    let mut cats: Vec<(&str, Vec<&Actor>)> = Vec::new();
    for a in ctx.uncollected() {
        if is_counting_wall(a) {
            continue; // added below from wall_status()
        }
        if let Some(c) = category(a) {
            match cats.iter_mut().find(|(n, _)| *n == c) {
                Some((_, v)) => v.push(a),
                None => cats.push((c, vec![a])),
            }
        }
    }
    if !closed_walls.is_empty() {
        cats.push(("Secret walls (count for the achievement)", closed_walls.clone()));
    }
    if !unknown_walls.is_empty() {
        cats.push(("Switch-operated secret walls (not tracked in the save)", unknown_walls.clone()));
    }
    let order = [
        "Skull Keys",
        "Secret walls (count for the achievement)",
        "Switch-operated secret walls (not tracked in the save)",
        "Throwing weapons",
        "Main weapons",
        "Treasures",
        "Accessories",
        "Upgrades & key items",
        "Chests",
        "Orbewa eggs",
        "Grimoire ink",
    ];
    cats.sort_by_key(|(n, _)| order.iter().position(|o| o == n).unwrap_or(99));
    for (name, actors) in &cats {
        let note: &dyn Fn(&Actor) -> String = if name.contains("ecret walls") { &wall_note } else { &no_note };
        let _ = write!(
            h,
            "<details{}><summary>{}<span class=count>{}</span></summary>{}</details>",
            if actors.len() <= 12 { " open" } else { "" },
            esc(name),
            actors.len(),
            actor_table(&ctx, actors, note)
        );
    }

    // Completed
    let _ = write!(h, "<h2>Completed</h2><details><summary>{} achievements<span class=count>show</span></summary><ul class=done>", done_ids.len());
    for id in &done_ids {
        let name = gd.challenges.get(id.as_str()).map(|d| d.name.clone()).unwrap_or_else(|| id.to_string());
        let _ = write!(h, "<li title=\"{}\">{}</li>", esc(id), esc(&name));
    }
    h += "</ul></details>";

    // Save files
    h += "<h2>Save files</h2>";
    let mut rows: Vec<String> = Vec::new();
    let mut worlds: Vec<&WorldSave> = sd.worlds.iter().collect();
    worlds.sort_by_key(|w| std::cmp::Reverse(w.last_save_ticks));
    for x in worlds {
        rows.push(format!(
            "<tr><td class=mono>{}</td><td>{}</td><td>{}</td><td class=num>{}</td><td class=num>{}</td><td>{}</td></tr>",
            esc(&x.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
            ticks_to_string(x.last_save_ticks),
            if x.suspend { "suspend" } else { "save" },
            hours(x.playtime_secs),
            x.level,
            if std::ptr::eq(x, w) { "<b>used for this report</b>" } else { "" }
        ));
    }
    h += &table("<th>File</th><th>Saved</th><th>Type</th><th class=num>Play time</th><th class=num>Level</th><th></th>", &rows);
    for e in &sd.errors {
        let _ = write!(h, "<p class=miss>{}</p>", esc(e));
    }

    h += "<footer>Generated by qd-save-report from the save files and the game's own data (.pak).</footer></main></body></html>";
    h
}
