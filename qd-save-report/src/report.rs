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
:root{--bg:#f6f5f1;--card:#ffffff;--ink:#1d1c1a;--muted:#6b675f;--line:#e3e0d8;--accent:#7a4fd6;--ok:#2f7d4f;--miss:#b4402a;--warn:#a26a00;--track:#ebe8e0;--tab:#ece9e1;color-scheme:light}
@media (prefers-color-scheme:dark){:root{--bg:#141317;--card:#1d1c22;--ink:#ecebf0;--muted:#9b98a6;--line:#2e2c35;--accent:#a88bff;--ok:#5cc286;--miss:#ff7a61;--warn:#e0b04a;--track:#2b2933;--tab:#24222a;color-scheme:dark}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:14px/1.45 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}
main{max-width:1180px;margin:0 auto;padding:24px 16px 64px}
h1{font-size:22px;margin:0 0 2px;letter-spacing:-.01em}
h2{font-size:16px;margin:24px 0 10px}
h3{font-size:15px;margin:0}
h4{font-size:14px;margin:14px 0 4px}
.meta{color:var(--muted);margin:0 0 14px;font-size:13px}
nav.tabs{display:flex;flex-wrap:wrap;gap:4px;border-bottom:1px solid var(--line);margin:0 0 16px;position:sticky;top:env(safe-area-inset-top,0px);background:var(--bg);padding-top:6px;z-index:2}
nav.tabs button{font:inherit;border:1px solid transparent;border-bottom:none;background:none;color:var(--muted);padding:7px 12px;border-radius:8px 8px 0 0;cursor:pointer}
nav.tabs button:hover{color:var(--ink);background:var(--tab)}
nav.tabs button[aria-selected=true]{color:var(--ink);background:var(--card);border-color:var(--line);margin-bottom:-1px;font-weight:600}
nav.tabs .n{font-size:12px;color:var(--muted);margin-left:5px;font-weight:400;font-variant-numeric:tabular-nums}
.panel[hidden]{display:none}
.tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(190px,1fr));gap:10px}
.tile{display:block;background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 12px;color:inherit;text-decoration:none}
a.tile:hover{border-color:var(--accent)}
.tile .l{color:var(--muted);font-size:12px}
.tile .v{font-size:20px;font-weight:650;font-variant-numeric:tabular-nums;line-height:1.3}
.tile .v small{font-size:13px;color:var(--muted);font-weight:400}
.tile .s{color:var(--muted);font-size:12px;margin-top:2px}
.tile.complete .v{color:var(--ok)}
.mini{height:5px;background:var(--track);border-radius:99px;overflow:hidden;margin-top:6px}
.mini div{height:100%;background:var(--accent)}
.tile.complete .mini div{background:var(--ok)}
.todo{list-style:none;margin:0;padding:0;display:grid;grid-template-columns:repeat(auto-fill,minmax(330px,1fr));gap:6px 18px}
.todo li{display:flex;align-items:center;gap:10px;padding:5px 0;border-bottom:1px solid var(--line)}
.todo li .t{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.todo li .p{font-size:12px;color:var(--muted);font-variant-numeric:tabular-nums;white-space:nowrap}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 16px;margin:10px 0}
.card .desc{color:var(--muted);margin:2px 0 8px}
.id{font:12px ui-monospace,SFMono-Regular,Menlo,monospace;color:var(--muted);margin-left:8px}
.progress{display:flex;align-items:center;gap:10px;margin:6px 0 4px}
.track{flex:1;max-width:340px;height:8px;background:var(--track);border-radius:99px;overflow:hidden}
.fill{height:100%;background:var(--accent);border-radius:99px}
.hint{color:var(--muted);margin:6px 0}
.bar{display:flex;flex-wrap:wrap;align-items:center;gap:8px 16px;margin:0 0 8px}
.bar .sum{font-weight:600}
.bar label{color:var(--muted);font-size:13px;display:flex;align-items:center;gap:6px;cursor:pointer;margin-left:auto}
.tablewrap{overflow-x:auto;margin:6px 0;background:var(--card);border:1px solid var(--line);border-radius:10px}
table{border-collapse:collapse;width:100%;font-size:13px}
th,td{text-align:left;padding:5px 10px;border-bottom:1px solid var(--line);white-space:nowrap;vertical-align:top}
tbody tr:last-child td{border-bottom:none}
th{font-weight:600;color:var(--muted);font-size:11px;text-transform:uppercase;letter-spacing:.04em;background:var(--card);position:sticky;top:0}
.num{text-align:right;font-variant-numeric:tabular-nums}
.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px}
.muted{color:var(--muted)}
.ok{color:var(--ok)}.miss{color:var(--miss)}.warn{color:var(--warn)}
tr.ok td:first-child{color:var(--ok)}
tr.miss td:first-child{color:var(--miss)}
.st{font-weight:600;font-size:12px}
.tag{display:inline-block;margin-left:6px;padding:0 6px;border:1px solid var(--line);border-radius:999px;font-size:11px;font-weight:400;color:var(--muted);vertical-align:1px}
.where div{white-space:nowrap}
.where .xyz{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px;color:var(--muted)}
.only-missing tr[data-s=ok]{display:none}
details{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:8px 14px;margin:8px 0}
summary{cursor:pointer;font-weight:600}
summary .count{color:var(--muted);font-weight:400;margin-left:6px}
details .tablewrap{border:none;background:none}
.done{display:grid;grid-template-columns:repeat(auto-fill,minmax(230px,1fr));gap:4px 16px;margin:8px 0 4px;padding:0;list-style:none}
.done li{padding:2px 0}.done li::before{content:"\2713  ";color:var(--ok)}
a{color:var(--accent)}
footer{margin-top:40px;color:var(--muted);font-size:12px}
@media (max-width:700px){
  main{padding:16px 12px 48px}
  nav.tabs{flex-wrap:nowrap;overflow-x:auto;scrollbar-width:none}
  nav.tabs button{flex:none}
  .tiles{grid-template-columns:repeat(2,minmax(0,1fr));gap:8px}
  .tile .v{font-size:18px}
  .todo{grid-template-columns:1fr}
}
"#;

const JS: &str = r#"
(function(){
  var tabs=[].slice.call(document.querySelectorAll('nav.tabs button'));
  var panels=[].slice.call(document.querySelectorAll('.panel'));
  function show(id,push){
    if(!document.getElementById('p-'+id))id='dashboard';
    tabs.forEach(function(b){b.setAttribute('aria-selected',b.dataset.tab===id?'true':'false')});
    panels.forEach(function(p){p.hidden=p.id!=='p-'+id});
    if(push){try{history.replaceState(null,'','#'+id)}catch(e){location.hash=id}}
  }
  tabs.forEach(function(b){b.addEventListener('click',function(){show(b.dataset.tab,true);window.scrollTo(0,0)})});
  document.querySelectorAll('a[data-go]').forEach(function(a){a.addEventListener('click',function(e){e.preventDefault();show(a.dataset.go,true);window.scrollTo(0,0)})});
  document.querySelectorAll('input[data-missing]').forEach(function(c){
    var p=c.closest('.panel');
    function apply(){p.classList.toggle('only-missing',c.checked)}
    c.addEventListener('change',apply);apply();
  });
  window.addEventListener('hashchange',function(){show(location.hash.slice(1),false)});
  show(location.hash.slice(1)||'dashboard',false);
})();
"#;

/// One collection tab: id, tab label, collected, total.
struct TabInfo {
    id: &'static str,
    label: &'static str,
    have: i64,
    total: i64,
    body: String,
}

fn status(ok: bool, yes: &str, no: &str) -> String {
    if ok {
        format!("<span class=\"st ok\">\u{2713} {}</span>", yes)
    } else {
        format!("<span class=\"st miss\">\u{2717} {}</span>", no)
    }
}

fn pct(have: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        ((have as f64 / total as f64) * 100.0).clamp(0.0, 100.0)
    }
}

/// Summary line + "only missing" switch at the top of a tab.
fn tab_head(summary: &str, hint: &str) -> String {
    format!(
        "<div class=bar><span class=sum>{}</span><label><input type=checkbox data-missing> only missing</label></div>{}",
        summary,
        if hint.is_empty() { String::new() } else { format!("<p class=hint>{}</p>", hint) }
    )
}

/// Table of placed actors with their state in the save: missing first, then by map.
fn world_tab_table(
    ctx: &Ctx,
    actors: &[&Actor],
    label: Option<&dyn Fn(&Actor) -> String>, // returns HTML (escape it yourself)
    is_ok: &dyn Fn(&Actor) -> Option<bool>,
    state: &dyn Fn(&Actor) -> String,
    extra_head: &str,
    extra: &dyn Fn(&Actor) -> String,
) -> String {
    let mut v: Vec<&&Actor> = actors.iter().collect();
    v.sort_by(|a, b| {
        // missing first, then unknown, then done
        let key = |x: Option<bool>| match x { Some(false) => 0, None => 1, Some(true) => 2 };
        let (ka, kb) = (key(is_ok(a)), key(is_ok(b)));
        ka.cmp(&kb).then(a.map.cmp(&b.map)).then(a.name.cmp(&b.name))
    });
    let rows: Vec<String> = v
        .iter()
        .map(|a| {
            let s = match is_ok(a) {
                Some(true) => "ok",
                Some(false) => "miss",
                None => "unknown",
            };
            format!(
                "<tr data-s={}><td>{}</td><td>{}</td>{}{}{}</tr>",
                s,
                label.map(|f| f(a)).unwrap_or_else(|| esc(&ctx.actor_label(a))),
                state(a),
                place_cells(a),
                loc_cells(a),
                extra(a)
            )
        })
        .collect();
    table(&format!("<th>Object</th><th>Status</th>{}{}", LOC_HEAD, extra_head), &rows)
}

/// Where an item can be found: its world pickups (with coordinates) and the shop.
fn where_cell(ctx: &Ctx, pickups: Option<&Vec<&Actor>>, item: &Item) -> String {
    let mut s = String::new();
    if let Some(ps) = pickups {
        let mut ps: Vec<&&Actor> = ps.iter().collect();
        ps.sort_by(|a, b| a.map.cmp(&b.map).then(a.name.cmp(&b.name)));
        for a in ps {
            let taken = ctx.w.touched(&a.guids);
            let xyz = a.loc.map(|[x, y, z]| format!("{:.0} {:.0} {:.0}", x, y, z)).unwrap_or_else(|| "?".into());
            let _ = write!(
                s,
                "<div>{} <span class=mono>{}</span> <span class=xyz>{}</span>{}</div>",
                esc(area(&a.map)),
                esc(&a.map),
                xyz,
                if taken { " <span class=ok title=\"picked up\">\u{2713}</span>" } else { "" }
            );
        }
    }
    for e in ctx.gd.shop.iter().filter(|e| e.grants.contains(&item.path)) {
        let gated = e.unlock_item.as_deref() == Some(item.path.as_str());
        let _ = write!(
            s,
            "<div>Umbada's shop, {} coins{}</div>",
            e.price,
            if gated { " <span class=muted>(listed after you found one)</span>" } else { "" }
        );
    }
    if s.is_empty() {
        s = "<span class=muted>not placed in the world (chest, boss, NPC or quest reward)</span>".into();
    }
    format!("<td class=where>{}</td>", s)
}

/// Item tab (weapons, throwables, accessories).
fn item_tab_rows(
    ctx: &Ctx,
    items: &[&Item],
    pickups: &HashMap<&str, Vec<&Actor>>,
    is_ok: &dyn Fn(&Item) -> bool,
    state: &dyn Fn(&Item) -> String,
    extra: &dyn Fn(&Item) -> String,
) -> Vec<String> {
    let mut v: Vec<&&Item> = items.iter().collect();
    v.sort_by(|a, b| is_ok(a).cmp(&is_ok(b)).then(a.name.cmp(&b.name)));
    v.iter()
        .map(|i| {
            let ok = is_ok(i);
            format!(
                "<tr data-s={}><td>{}</td><td>{}</td>{}{}</tr>",
                if ok { "ok" } else { "miss" },
                esc(&i.name),
                state(i),
                extra(i),
                where_cell(ctx, pickups.get(i.path.as_str()), i)
            )
        })
        .collect()
}

fn is_meal(a: &Actor) -> bool {
    a.class_pkg.starts_with("/Game/Items/Upgrades/") && a.class.contains("Pedestal")
}

fn tile(label: &str, have: i64, total: i64, sub: &str, go: Option<&str>) -> String {
    let done = total > 0 && have >= total;
    let inner = format!(
        "<div class=l>{}</div><div class=v>{} <small>/ {}</small></div><div class=mini><div style=\"width:{:.1}%\"></div></div>{}",
        esc(label),
        have,
        total,
        pct(have, total),
        if sub.is_empty() { String::new() } else { format!("<div class=s>{}</div>", sub) }
    );
    match go {
        Some(id) => format!("<a class=\"tile{}\" href=\"#{}\" data-go=\"{}\">{}</a>", if done { " complete" } else { "" }, id, id, inner),
        None => format!("<div class=\"tile{}\">{}</div>", if done { " complete" } else { "" }, inner),
    }
}

fn plain_tile(label: &str, value: &str) -> String {
    format!("<div class=tile><div class=l>{}</div><div class=v>{}</div></div>", esc(label), value)
}

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
    let target = |id: &str, key: &str, fallback: i64| -> i64 {
        gd.challenges
            .get(id)
            .and_then(|d| d.nsd.iter().find(|(k, _)| k == key).map(|(_, t)| *t))
            .unwrap_or(fallback)
    };

    // World pickups per item path (weapons, throwables, accessories, ...).
    let mut pickups: HashMap<&str, Vec<&Actor>> = HashMap::new();
    for a in &gd.actors {
        if let Some(i) = ctx.pickup_item(a) {
            pickups.entry(i.path.as_str()).or_default().push(a);
        }
    }

    let mut tabs: Vec<TabInfo> = Vec::new();

    // --- Skull Keys -------------------------------------------------------
    {
        let all: Vec<&Actor> = gd.actors.iter().filter(|a| a.class.starts_with("SkullKeyRelic")).collect();
        let have = all.iter().filter(|a| w.touched(&a.guids)).count() as i64;
        let turned = w.nsd("SkullsTurnedIn");
        let held = gd
            .items
            .keys()
            .find(|k| k.ends_with("SkullKeyRelic_Data.SkullKeyRelic_Data"))
            .map(|k| w.count(k))
            .unwrap_or(0);
        let mut body = tab_head(
            &format!("{} of {} Skull Keys picked up", have, all.len()),
            &format!("Turned in at the Secret Skull Door: <b>{}</b> \u{00b7} carried: <b>{}</b>", turned, held),
        );
        body += &world_tab_table(
            &ctx,
            &all,
            None,
            &|a| Some(w.touched(&a.guids)),
            &|a| status(w.touched(&a.guids), "collected", "missing"),
            "",
            &|_| String::new(),
        );
        let rows: Vec<String> = gd
            .skull_rewards
            .iter()
            .map(|(n, r)| {
                format!(
                    "<tr data-s={}><td class=num>{}</td><td>{}</td><td>{}</td></tr>",
                    if turned >= *n { "ok" } else { "miss" },
                    n,
                    esc(&ctx.class_path_label(r)),
                    status(turned >= *n, "received", "not yet")
                )
            })
            .collect();
        if !rows.is_empty() {
            body += "<h2>Skull Door rewards</h2>";
            body += &table("<th class=num>Skulls</th><th>Reward</th><th>Status</th>", &rows);
        }
        tabs.push(TabInfo { id: "skulls", label: "Skull Keys", have, total: all.len() as i64, body });
    }

    // --- Eggs ---------------------------------------------------------------
    {
        let all: Vec<&Actor> = gd.actors.iter().filter(|a| a.class.contains("FishWife_Egg")).collect();
        let have = all.iter().filter(|a| w.touched(&a.guids)).count() as i64;
        let counter = w.nsd("FishWifeTotalEggs");
        let need = target("cha_Quest_Fish", "FishWifeTotalEggs", all.len() as i64);
        let mut body = tab_head(
            &format!("{} of {} Orbewa eggs picked up", have, all.len()),
            &format!("Eggs brought back (game counter <span class=mono>FishWifeTotalEggs</span>): <b>{}</b> of {} for the achievement.", counter, need),
        );
        body += &world_tab_table(
            &ctx,
            &all,
            None,
            &|a| Some(w.touched(&a.guids)),
            &|a| status(w.touched(&a.guids), "collected", "missing"),
            "",
            &|_| String::new(),
        );
        tabs.push(TabInfo { id: "eggs", label: "Eggs", have, total: all.len() as i64, body });
    }

    // --- Weapons --------------------------------------------------------------
    {
        let items: Vec<&Item> = gd.items.values().filter(|i| i.has_tag("ItemTags.AchievementWeapon")).collect();
        let have = items.iter().filter(|i| w.ever_had(&i.path)).count() as i64;
        let upgraded = items.iter().filter(|i| w.weapon_upgrades.contains_key(&i.path)).count();
        let mut body = tab_head(
            &format!("{} of {} weapons obtained \u{00b7} {} upgraded", have, items.len(), upgraded),
            "Level is the current weapon level; the upgrade column shows Ossi's upgrade machine status and whether the weapon's upgrade data is installed.",
        );
        let rows = item_tab_rows(
            &ctx,
            &items,
            &pickups,
            &|i| w.ever_had(&i.path),
            &|i| status(w.ever_had(&i.path), "obtained", "missing"),
            &|i| {
                let lv = w.weapon_levels.get(&i.path).copied();
                let max = i.max_level.map(|m| m - 1);
                let level = match (lv, max) {
                    (Some(l), Some(m)) => format!("{} / {}", l, m),
                    (Some(l), None) => l.to_string(),
                    (None, Some(m)) => format!("\u{2013} / {}", m),
                    (None, None) => "\u{2013}".into(),
                };
                let upg = if w.weapon_upgrades.contains_key(&i.path) {
                    "<span class=ok>upgraded</span>".to_string()
                } else if let Some(idx) = i.upgrade_index {
                    let installed = w.nsd(&format!("WeaponUpgradeUnlock_{:02}", idx)) > 0;
                    format!("#{} {}", idx, if installed { "<span class=warn>data installed</span>" } else { "<span class=muted>no data</span>" })
                } else {
                    "<span class=muted>\u{2013}</span>".into()
                };
                format!("<td class=num>{}</td><td>{}</td>", level, upg)
            },
        );
        body += &table("<th>Weapon</th><th>Status</th><th class=num>Level</th><th>Upgrade</th><th>Where</th>", &rows);
        tabs.push(TabInfo { id: "weapons", label: "Weapons", have, total: items.len() as i64, body });
    }

    // --- Accessories -----------------------------------------------------------
    {
        let items: Vec<&Item> = gd.items.values().filter(|i| i.has_tag("ItemTags.AchievementAccessory")).collect();
        let have = items.iter().filter(|i| w.ever_had(&i.path)).count() as i64;
        let mut body = tab_head(&format!("{} of {} accessories obtained", have, items.len()), "");
        let rows = item_tab_rows(
            &ctx,
            &items,
            &pickups,
            &|i| w.ever_had(&i.path),
            &|i| status(w.ever_had(&i.path), "obtained", "missing"),
            &|_| String::new(),
        );
        body += &table("<th>Accessory</th><th>Status</th><th>Where</th>", &rows);
        tabs.push(TabInfo { id: "accessories", label: "Accessories", have, total: items.len() as i64, body });
    }

    // --- Throwing weapons -------------------------------------------------------
    {
        let items: Vec<&Item> = gd.items.values().filter(|i| i.has_tag("ItemTags.AchievementThrowables")).collect();
        let have = items.iter().filter(|i| w.ach_throwables.contains(&i.path)).count() as i64;
        let mut body = tab_head(
            &format!("{} of {} throwing weapons registered", have, items.len()),
            "The achievement counts a throwing weapon once it has been picked up (the save keeps a separate list for this), not what is in the inventory now.",
        );
        let rows = item_tab_rows(
            &ctx,
            &items,
            &pickups,
            &|i| w.ach_throwables.contains(&i.path),
            &|i| status(w.ach_throwables.contains(&i.path), "registered", "missing"),
            &|i| format!("<td class=num>{}</td>", w.count(&i.path)),
        );
        body += &table("<th>Throwing weapon</th><th>Status</th><th class=num>Carried</th><th>Where</th>", &rows);
        tabs.push(TabInfo { id: "throwables", label: "Throwing weapons", have, total: items.len() as i64, body });
    }

    // --- Meal tickets -------------------------------------------------------------
    {
        let all: Vec<&Actor> = gd.actors.iter().filter(|a| is_meal(a)).collect();
        let have = all.iter().filter(|a| w.touched(&a.guids)).count() as i64;
        let counter = w.nsd("MealTicketsCollected");
        let need = target("cha_Quest_Chef", "MealTicketsCollected", 60);
        let mut body = tab_head(
            &format!("{} of {} meals eaten (each leaves a Meal Ticket)", have, all.len()),
            &format!(
                "Tickets given to Chef Gagi (game counter <span class=mono>MealTicketsCollected</span>): <b>{}</b> of {} for the achievement. Rewards received: <b>{}</b>.",
                counter,
                need,
                w.nsd("ChefQuestRewardCount")
            ),
        );
        body += &world_tab_table(
            &ctx,
            &all,
            None,
            &|a| Some(w.touched(&a.guids)),
            &|a| status(w.touched(&a.guids), "eaten", "missing"),
            "",
            &|_| String::new(),
        );
        tabs.push(TabInfo { id: "meals", label: "Meal tickets", have, total: all.len() as i64, body });
    }

    // --- Hidden doors ----------------------------------------------------------------
    {
        let all: Vec<&Actor> = gd.actors.iter().filter(|a| a.class.contains("SecretWall")).collect();
        let counting = all.iter().filter(|a| !a.no_count).count() as i64;
        let opened = w.nsd("TotalSecretDoorsOpened");
        let need = target("cha_Challenge_AllSecretDoors", "TotalSecretDoorsOpened", counting);
        let (closed, _) = ctx.wall_status();
        let all_counting_open = opened >= counting;
        // Per wall: Some(true) open, Some(false) closed, None = not recorded in the save.
        let wall_ok = |a: &Actor| -> Option<bool> {
            if w.touched(&a.guids) {
                Some(true)
            } else if !a.no_count && all_counting_open {
                Some(true) // the game's own counter covers every counting wall
            } else if a.activatable {
                None
            } else {
                Some(false)
            }
        };
        let have = all.iter().filter(|a| !a.no_count && wall_ok(a) == Some(true)).count() as i64;
        let mut hint = format!(
            "Game counter <span class=mono>TotalSecretDoorsOpened</span>: <b>{}</b> of {} needed for the achievement. {} walls count towards it, {} don't (they still hide things).",
            opened,
            need,
            counting,
            all.len() as i64 - counting
        );
        if all_counting_open && need > counting {
            let _ = write!(
                hint,
                " <b>Every counting wall is open</b>; the achievement asks for {} but only {} exist, which looks like a bug in the game.",
                need, counting
            );
        } else if !closed.is_empty() {
            let _ = write!(hint, " {} counting walls are still closed.", closed.len());
        }
        hint += " Walls opened by a switch don't store their state in the save, so they show as \u{201c}unknown\u{201d} unless the counter proves them open.";
        let mut body = tab_head(&format!("{} of {} counting walls open", have.min(counting), counting), &hint);
        body += &world_tab_table(
            &ctx,
            &all,
            Some(&|a: &Actor| {
                let mut s = esc(&a.name);
                if a.no_count {
                    s += " <span class=tag title=\"Doesn't count towards the All Secret Doors achievement\">not counted</span>";
                }
                s
            }),
            &|a| wall_ok(a),
            &|a| match wall_ok(a) {
                Some(true) => status(true, "open", ""),
                Some(false) => status(false, "", "closed"),
                None => "<span class=\"st warn\">? unknown</span>".into(),
            },
            "<th>Counts</th><th>Opens by</th>",
            &|a| {
                format!(
                    "<td>{}</td><td class=muted>{}</td>",
                    if a.no_count { "<span class=muted>no</span>" } else { "yes" },
                    wall_note(a)
                )
            },
        );
        tabs.push(TabInfo { id: "doors", label: "Hidden doors", have: have.min(counting), total: counting, body });
    }

    // --- Achievements -------------------------------------------------------------------
    let mut ach = String::new();
    let _ = write!(ach, "<h2>Still to do ({})</h2>", missing_ids.len());
    if missing_ids.is_empty() {
        ach += "<p>Every achievement is complete.</p>";
    }
    let mut todo_list = String::new();
    for id in &missing_ids {
        match gd.challenges.get(id.as_str()) {
            Some(d) => {
                let (cur, max) = ctx.progress(d);
                let _ = write!(
                    ach,
                    "<section class=card id=\"a-{}\"><h3>{}<span class=id>{}</span></h3><p class=desc>{}</p>{}{}</section>",
                    esc(id),
                    esc(&d.name),
                    esc(id),
                    esc(&d.desc),
                    bar(cur, max),
                    details(&ctx, d)
                );
                let p = match (cur, max) {
                    (Some(c), Some(m)) if m > 0 => format!("{} / {}", c, m),
                    _ => String::new(),
                };
                let _ = write!(
                    todo_list,
                    "<li><span class=t title=\"{}\">{}</span><span class=p>{}</span></li>",
                    esc(&d.desc),
                    esc(&d.name),
                    p
                );
            }
            None => {
                let _ = write!(ach, "<section class=card><h3>{}</h3><p class=desc>No challenge definition found.</p></section>", esc(id));
                let _ = write!(todo_list, "<li><span class=t>{}</span></li>", esc(id));
            }
        }
    }
    let _ = write!(ach, "<h2>Completed ({})</h2><ul class=done>", done_ids.len());
    for id in &done_ids {
        let name = gd.challenges.get(id.as_str()).map(|d| d.name.clone()).unwrap_or_else(|| id.to_string());
        let _ = write!(ach, "<li title=\"{}\">{}</li>", esc(id), esc(&name));
    }
    ach += "</ul>";

    // --- More: the remaining world categories + save files ---------------------------------
    let mut more = String::from(
        "<p class=hint>Everything else that is still in the world. Coordinates are Unreal world units (X / Y / Z), the same values the CoordsDisplay mod shows in game. Some pickups exist in several variants at the same spot; collecting one is enough.</p>",
    );
    let mut cats: Vec<(&str, Vec<&Actor>)> = Vec::new();
    for a in ctx.uncollected() {
        let c = match category(a) {
            Some(c) => c,
            None => continue,
        };
        // Covered by their own tabs.
        if matches!(c, "Skull Keys" | "Orbewa eggs" | "Throwing weapons" | "Accessories") || c.contains("ecret walls") || is_meal(a) {
            continue;
        }
        match cats.iter_mut().find(|(n, _)| *n == c) {
            Some((_, v)) => v.push(a),
            None => cats.push((c, vec![a])),
        }
    }
    let order = ["Treasures", "Main weapons", "Upgrades & key items", "Chests", "Grimoire ink"];
    cats.sort_by_key(|(n, _)| order.iter().position(|o| o == n).unwrap_or(99));
    if cats.is_empty() {
        more += "<p>Nothing else left in the world.</p>";
    }
    for (name, actors) in &cats {
        let _ = write!(
            more,
            "<details{}><summary>{}<span class=count>{}</span></summary>{}</details>",
            if actors.len() <= 12 { " open" } else { "" },
            esc(name),
            actors.len(),
            actor_table(&ctx, actors, &no_note)
        );
    }
    more += "<h2>Save files</h2>";
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
    more += &table("<th>File</th><th>Saved</th><th>Type</th><th class=num>Play time</th><th class=num>Level</th><th></th>", &rows);
    for e in &sd.errors {
        let _ = write!(more, "<p class=miss>{}</p>", esc(e));
    }

    // --- Dashboard ------------------------------------------------------------------------------
    let coins = gd.items.keys().find(|k| k.ends_with("/Coins/Coin.Coin")).map(|k| w.count(k)).unwrap_or(0);
    let mut dash = String::from("<div class=tiles>");
    dash += &tile("Achievements", done_ids.len() as i64, total as i64, "", Some("achievements"));
    for t in &tabs {
        let sub = match t.id {
            "skulls" => format!("{} turned in", w.nsd("SkullsTurnedIn")),
            "eggs" => format!("{} / {} brought back", w.nsd("FishWifeTotalEggs"), target("cha_Quest_Fish", "FishWifeTotalEggs", 50)),
            "meals" => format!("{} / {} tickets handed in", w.nsd("MealTicketsCollected"), target("cha_Quest_Chef", "MealTicketsCollected", 60)),
            "doors" => format!("counter {} / {}", w.nsd("TotalSecretDoorsOpened"), target("cha_Challenge_AllSecretDoors", "TotalSecretDoorsOpened", t.total)),
            "weapons" => format!("{} upgraded", gd.items.values().filter(|i| i.has_tag("ItemTags.AchievementWeapon") && w.weapon_upgrades.contains_key(&i.path)).count()),
            _ => String::new(),
        };
        dash += &tile(t.label, t.have, t.total, &sub, Some(t.id));
    }
    dash += &plain_tile("Play time", &hours(w.playtime_secs));
    dash += &plain_tile("Player level", &w.level.to_string());
    dash += &plain_tile("Coins", &coins.to_string());
    dash += "</div>";
    let _ = write!(
        dash,
        "<h2>Still to do <span class=muted>({})</span> \u{00b7} <a href=\"#achievements\" data-go=achievements>details</a></h2>",
        missing_ids.len()
    );
    if missing_ids.is_empty() {
        dash += "<p>Every achievement is complete.</p>";
    } else {
        let _ = write!(dash, "<ul class=todo>{}</ul>", todo_list);
    }

    // --- Page --------------------------------------------------------------------------------------
    let mut h = String::new();
    let _ = write!(
        h,
        "<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Queen's Domain save report</title><style>{}</style></head><body><main>",
        CSS
    );
    let _ = write!(
        h,
        "<h1>Queen's Domain \u{2014} save report</h1><p class=meta><span class=mono>{}</span>{} \u{00b7} saved {} \u{00b7} {} played \u{00b7} level {}</p>",
        esc(&w.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        if w.suspend { " (suspend save)" } else { "" },
        ticks_to_string(w.last_save_ticks),
        hours(w.playtime_secs),
        w.level
    );

    h += "<nav class=tabs role=tablist>";
    let _ = write!(h, "<button role=tab data-tab=dashboard>Dashboard</button>");
    let _ = write!(
        h,
        "<button role=tab data-tab=achievements>Achievements<span class=n>{}/{}</span></button>",
        done_ids.len(),
        total
    );
    for t in &tabs {
        let _ = write!(h, "<button role=tab data-tab={}>{}<span class=n>{}/{}</span></button>", t.id, t.label, t.have, t.total);
    }
    h += "<button role=tab data-tab=more>More</button></nav>";

    let _ = write!(h, "<section class=panel id=p-dashboard role=tabpanel>{}</section>", dash);
    let _ = write!(h, "<section class=panel id=p-achievements role=tabpanel hidden>{}</section>", ach);
    for t in &tabs {
        let _ = write!(h, "<section class=panel id=p-{} role=tabpanel hidden>{}</section>", t.id, t.body);
    }
    let _ = write!(h, "<section class=panel id=p-more role=tabpanel hidden>{}</section>", more);

    let _ = write!(
        h,
        "<footer>Generated by qd-save-report from the save files and the game's own data (.pak). Folder <span class=mono>{}</span></footer></main><script>{}</script></body></html>",
        esc(&sd.dir.display().to_string()),
        JS
    );
    h
}
