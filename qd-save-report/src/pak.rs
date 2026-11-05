//! Minimal reader for UE 4.25+ (.pak version 10/11) archives with an
//! unencrypted index and uncompressed entries, which is how Queen's Domain
//! ships its content.

use crate::cursor::{Cur, Res};
use crate::uasset::Asset;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const FOOTER_SIZE: u64 = 221;
const PAK_MAGIC: u32 = 0x5A6F12E1;
/// Serialized FPakEntry header for an uncompressed, unencrypted entry.
const ENTRY_HEADER: u64 = 8 + 8 + 8 + 4 + 20 + 1 + 4;

pub struct Entry {
    pub offset: u64,
    pub size: u64,
    pub compressed: bool,
    pub encrypted: bool,
}

pub struct Pak {
    file: RefCell<File>,
    /// Keyed by path relative to the mount point, e.g.
    /// `QueensDomain/Content/Blueprints/Shop/Shop.uasset`.
    pub entries: HashMap<String, Entry>,
    /// Project directory name inside the pak (`QueensDomain`).
    pub project: String,
}

fn read_at(f: &mut File, off: u64, len: u64) -> Res<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn decode_entry(enc: &[u8], off: usize) -> Res<Entry> {
    let mut c = Cur::at(enc, off);
    let bits = c.u32()?;
    if bits & 0x3f == 0x3f {
        c.u32()?; // explicit compression block size
    }
    let encrypted = (bits >> 22) & 1 == 1;
    let method = (bits >> 23) & 0x3f;
    let read = |c: &mut Cur, is32: bool| -> Res<u64> {
        if is32 { Ok(c.u32()? as u64) } else { c.u64() }
    };
    let offset = read(&mut c, (bits >> 31) & 1 == 1)?;
    let size = read(&mut c, (bits >> 30) & 1 == 1)?;
    Ok(Entry { offset, size, compressed: method != 0, encrypted })
}

impl Pak {
    pub fn open(path: &Path) -> Res<Pak> {
        let mut f = File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        if len < FOOTER_SIZE {
            return Err("file too small to be a pak".into());
        }
        let footer = read_at(&mut f, len - FOOTER_SIZE, FOOTER_SIZE)?;
        let mut c = Cur::new(&footer);
        c.skip(16)?; // encryption key guid
        let index_encrypted = c.u8()?;
        let magic = c.u32()?;
        let version = c.i32()?;
        let index_off = c.u64()?;
        let index_size = c.u64()?;
        if magic != PAK_MAGIC {
            return Err("unsupported pak format (expected a UE 4.25+ v11 footer)".into());
        }
        if version < 10 {
            return Err(format!("unsupported pak version {}", version));
        }
        if index_encrypted != 0 {
            return Err("pak index is encrypted".into());
        }

        let index = read_at(&mut f, index_off, index_size)?;
        let mut c = Cur::new(&index);
        let mount = c.fstring()?;
        c.i32()?; // entry count
        c.u64()?; // path hash seed
        if c.i32()? != 0 {
            c.skip(8 + 8 + 20)?; // path hash index
        }
        if c.i32()? == 0 {
            return Err("pak has no full directory index".into());
        }
        let dir_off = c.u64()?;
        let dir_size = c.u64()?;
        c.skip(20)?;
        let enc_size = c.i32()? as usize;
        let encoded = c.take(enc_size)?.to_vec();

        let dir_index = read_at(&mut f, dir_off, dir_size)?;
        let mut d = Cur::new(&dir_index);
        let mount = mount.trim_start_matches("../").to_string();
        let mut entries = HashMap::new();
        let dirs = d.i32()?;
        for _ in 0..dirs {
            let dir = d.fstring()?;
            let files = d.i32()?;
            for _ in 0..files {
                let name = d.fstring()?;
                let eo = d.i32()?;
                if eo < 0 {
                    continue; // non-encoded entries are not used by this game
                }
                let entry = decode_entry(&encoded, eo as usize)?;
                entries.insert(format!("{}{}{}", mount, dir.trim_start_matches('/'), name), entry);
            }
        }

        let project = entries
            .keys()
            .find_map(|k| k.strip_suffix("/Config/DefaultEngine.ini").map(str::to_string))
            .unwrap_or_else(|| "QueensDomain".to_string());

        Ok(Pak { file: RefCell::new(f), entries, project })
    }

    pub fn read(&self, path: &str) -> Res<Vec<u8>> {
        let e = self.entries.get(path).ok_or_else(|| format!("{} not found in pak", path))?;
        if e.compressed || e.encrypted {
            return Err(format!("{} is compressed or encrypted (unsupported)", path));
        }
        read_at(&mut self.file.borrow_mut(), e.offset + ENTRY_HEADER, e.size)
    }

    /// `/Game/Foo/Bar` -> `QueensDomain/Content/Foo/Bar`
    pub fn content_path(&self, game_path: &str) -> String {
        let rest = game_path.strip_prefix("/Game/").unwrap_or(game_path.trim_start_matches('/'));
        format!("{}/Content/{}", self.project, rest)
    }

    /// `QueensDomain/Content/Foo/Bar.uasset` -> `/Game/Foo/Bar`
    pub fn game_path(&self, pak_path: &str) -> Option<String> {
        let prefix = format!("{}/Content/", self.project);
        let rest = pak_path.strip_prefix(&prefix)?;
        let rest = rest.rsplit_once('.').map(|(a, _)| a).unwrap_or(rest);
        Some(format!("/Game/{}", rest))
    }

    /// Loads a package by game path (`/Game/...`, without extension) or by
    /// object path (`/Game/.../Name.Name`).
    pub fn asset(&self, game_path: &str) -> Res<Asset> {
        let pkg = game_path.split('.').next().unwrap_or(game_path);
        let base = self.content_path(pkg);
        let header = if self.entries.contains_key(&format!("{}.umap", base)) {
            self.read(&format!("{}.umap", base))?
        } else {
            self.read(&format!("{}.uasset", base))?
        };
        let exp = self.read(&format!("{}.uexp", base)).unwrap_or_default();
        Asset::parse(header, &exp)
    }
}
