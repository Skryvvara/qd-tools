//! Parser for cooked UE 4.27 packages (.uasset/.umap + .uexp) that use
//! classic tagged property serialization.

use crate::cursor::{Cur, Res};
use crate::value::Value;

pub struct Import {
    pub class_name: String,
    pub name: String,
    pub outer: i32,
}

pub struct Export {
    pub class: i32,
    pub outer: i32,
    pub name: String,
    pub size: usize,
    pub off: usize,
}

pub struct Asset {
    pub b: Vec<u8>,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

const PACKAGE_TAG: u32 = 0x9E2A83C1;
const PKG_FILTER_EDITOR_ONLY: u32 = 0x8000_0000;

impl Asset {
    pub fn parse(header: Vec<u8>, exp: &[u8]) -> Res<Asset> {
        let mut b = header;
        b.extend_from_slice(exp);

        let (names, imports, exports) = {
            let mut c = Cur::new(&b);
            if c.u32()? != PACKAGE_TAG {
                return Err("not an Unreal package".into());
            }
            let legacy = c.i32()?;
            if legacy != -4 {
                c.i32()?; // legacy UE3 version
            }
            c.i32()?; // UE4 file version
            c.i32()?; // licensee version
            let custom = c.i32()?;
            if !(0..=1000).contains(&custom) {
                return Err("bad custom version count".into());
            }
            c.skip(custom as usize * 20)?;
            c.i32()?; // total header size
            c.fstring()?; // folder name
            let flags = c.u32()?;
            let name_count = c.i32()?;
            let name_off = c.i32()?;
            if flags & PKG_FILTER_EDITOR_ONLY == 0 {
                c.fstring()?; // localization id
            }
            c.i32()?; // gatherable text count
            c.i32()?; // gatherable text offset
            let export_count = c.i32()?;
            let export_off = c.i32()?;
            let import_count = c.i32()?;
            let import_off = c.i32()?;

            let mut names = Vec::with_capacity(name_count.max(0) as usize);
            let mut c = Cur::at(&b, name_off as usize);
            for _ in 0..name_count {
                names.push(c.fstring()?);
                c.skip(4)?; // hashes
            }
            let fname = |i: i32, n: i32| -> String {
                let base = names.get(i as usize).cloned().unwrap_or_default();
                if n > 0 { format!("{}_{}", base, n - 1) } else { base }
            };

            let mut imports = Vec::with_capacity(import_count.max(0) as usize);
            let mut c = Cur::at(&b, import_off as usize);
            for _ in 0..import_count {
                c.skip(8)?; // class package
                let cn = c.i32()?;
                let cnn = c.i32()?;
                let outer = c.i32()?;
                let on = c.i32()?;
                let onn = c.i32()?;
                imports.push(Import { class_name: fname(cn, cnn), name: fname(on, onn), outer });
            }

            let mut exports = Vec::with_capacity(export_count.max(0) as usize);
            for i in 0..export_count.max(0) as usize {
                let mut c = Cur::at(&b, export_off as usize + i * 104);
                let class = c.i32()?;
                c.i32()?; // super
                c.i32()?; // template
                let outer = c.i32()?;
                let ni = c.i32()?;
                let nn = c.i32()?;
                c.u32()?; // object flags
                let size = c.i64()?;
                let off = c.i64()?;
                exports.push(Export { class, outer, name: fname(ni, nn), size: size.max(0) as usize, off: off.max(0) as usize });
            }
            (names, imports, exports)
        };

        Ok(Asset { b, names, imports, exports })
    }

    pub fn fname(&self, i: i32, n: i32) -> Res<String> {
        let base = self.names.get(i as usize).ok_or_else(|| format!("bad name index {}", i))?;
        Ok(if n > 0 { format!("{}_{}", base, n - 1) } else { base.clone() })
    }

    /// Full object path for an import (`/Game/Pkg/Asset.Asset`) when its outer
    /// is a package, otherwise just its name.
    pub fn import_path(&self, i: usize) -> String {
        let Some(imp) = self.imports.get(i) else { return String::new() };
        if imp.outer < 0 {
            if let Some(o) = self.imports.get((-imp.outer - 1) as usize) {
                if o.class_name == "Package" {
                    return format!("{}.{}", o.name, imp.name);
                }
            }
        }
        imp.name.clone()
    }

    pub fn obj_ref(&self, idx: i32) -> Value {
        if idx < 0 {
            Value::Str(self.import_path((-idx - 1) as usize))
        } else if idx > 0 {
            self.exports.get(idx as usize - 1).map(|e| Value::Str(e.name.clone())).unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }

    /// (class name, class object path) of an export.
    pub fn class_of(&self, e: &Export) -> (String, String) {
        if e.class < 0 {
            let i = (-e.class - 1) as usize;
            let name = self.imports.get(i).map(|x| x.name.clone()).unwrap_or_default();
            (name, self.import_path(i))
        } else if e.class > 0 {
            let name = self.exports.get(e.class as usize - 1).map(|x| x.name.clone()).unwrap_or_default();
            (name.clone(), name)
        } else {
            (String::new(), String::new())
        }
    }

    pub fn export_props(&self, e: &Export) -> Value {
        if e.off >= self.b.len() {
            return Value::Null;
        }
        let mut p = Parser { a: self, c: Cur::at(&self.b, e.off) };
        p.props(Some(e.off + e.size)).unwrap_or(Value::Null)
    }

    pub fn parser_at(&self, off: usize) -> Parser<'_> {
        Parser { a: self, c: Cur::at(&self.b, off) }
    }
}

pub struct Parser<'a> {
    a: &'a Asset,
    pub c: Cur<'a>,
}

impl<'a> Parser<'a> {
    fn name(&mut self) -> Res<String> {
        let i = self.c.i32()?;
        let n = self.c.i32()?;
        if i < 0 {
            return Err("negative name index".into());
        }
        self.a.fname(i, n)
    }

    fn end_of(&self, size: usize) -> Res<usize> {
        let end = self.c.p + size;
        if end > self.c.b.len() { Err("property overruns package".into()) } else { Ok(end) }
    }

    pub fn props(&mut self, end: Option<usize>) -> Res<Value> {
        let mut out = Vec::new();
        loop {
            if let Some(e) = end {
                if self.c.p >= e {
                    break;
                }
            }
            let n = self.name()?;
            if n == "None" {
                break;
            }
            let t = self.name()?;
            let size = self.c.i32()?;
            let idx = self.c.i32()?;
            if size < 0 {
                return Err("negative property size".into());
            }
            let key = if idx == 0 { n } else { format!("{}[{}]", n, idx) };
            let v = self.prop(&t, size as usize)?;
            out.push((key, v));
        }
        Ok(Value::Struct(out))
    }

    fn prop(&mut self, t: &str, size: usize) -> Res<Value> {
        match t {
            "BoolProperty" => {
                let v = self.c.u8()?;
                self.c.u8()?;
                Ok(Value::Bool(v != 0))
            }
            "StructProperty" => {
                let st = self.name()?;
                self.c.skip(16)?;
                self.c.u8()?;
                let end = self.end_of(size)?;
                let v = self.struct_val(&st).unwrap_or(Value::Null);
                self.c.p = end;
                Ok(v)
            }
            "ByteProperty" => {
                let en = self.name()?;
                self.c.u8()?;
                let end = self.end_of(size)?;
                let v = if en == "None" {
                    if size == 1 { Value::Int(self.c.u8()? as i64) } else { Value::Null }
                } else {
                    self.name().map(Value::Str).unwrap_or(Value::Null)
                };
                self.c.p = end;
                Ok(v)
            }
            "EnumProperty" => {
                self.name()?;
                self.c.u8()?;
                let end = self.end_of(size)?;
                let v = self.name().map(Value::Str).unwrap_or(Value::Null);
                self.c.p = end;
                Ok(v)
            }
            "MapProperty" => {
                let kt = self.name()?;
                let vt = self.name()?;
                self.c.u8()?;
                let end = self.end_of(size)?;
                let v = self.map_val(&kt, &vt).unwrap_or(Value::Null);
                self.c.p = end;
                Ok(v)
            }
            "ArrayProperty" | "SetProperty" => {
                let it = self.name()?;
                self.c.u8()?;
                let end = self.end_of(size)?;
                let v = self.array_val(&it, t == "SetProperty").unwrap_or(Value::Null);
                self.c.p = end;
                Ok(v)
            }
            _ => {
                self.c.u8()?;
                let end = self.end_of(size)?;
                let v = self.simple(t).unwrap_or(Value::Null);
                self.c.p = end;
                Ok(v)
            }
        }
    }

    fn count(&mut self) -> Res<usize> {
        let n = self.c.i32()?;
        if n < 0 || n as usize > self.c.remaining() {
            return Err(format!("implausible element count {}", n));
        }
        Ok(n as usize)
    }

    fn map_val(&mut self, kt: &str, vt: &str) -> Res<Value> {
        if self.c.i32()? != 0 {
            return Err("maps with removed keys are not supported".into());
        }
        let n = self.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let k = if kt == "StructProperty" { self.props(None)? } else { self.simple(kt)? };
            let v = if vt == "StructProperty" { self.props(None)? } else { self.simple(vt)? };
            out.push((k, v));
        }
        Ok(Value::Map(out))
    }

    fn array_val(&mut self, it: &str, is_set: bool) -> Res<Value> {
        if is_set && self.c.i32()? != 0 {
            return Err("sets with removed keys are not supported".into());
        }
        let n = self.count()?;
        let mut out = Vec::with_capacity(n);
        if it == "StructProperty" {
            if is_set {
                return Err("struct sets are not supported".into());
            }
            self.name()?;
            self.name()?;
            self.c.skip(8)?;
            let st = self.name()?;
            self.c.skip(17)?;
            for _ in 0..n {
                out.push(self.struct_val(&st)?);
            }
        } else {
            for _ in 0..n {
                out.push(self.simple(it)?);
            }
        }
        Ok(Value::Array(out))
    }

    fn text(&mut self) -> Res<Value> {
        self.c.i32()?; // flags
        let history = self.c.u8()?;
        match history {
            255 => {
                let has = self.c.i32()?;
                Ok(Value::Str(if has != 0 { self.c.fstring()? } else { String::new() }))
            }
            0 => {
                self.c.fstring()?; // namespace
                self.c.fstring()?; // key
                Ok(Value::Str(self.c.fstring()?))
            }
            11 => {
                self.name()?; // string table id
                Ok(Value::Str(format!("@{}", self.c.fstring()?)))
            }
            _ => Err(format!("unsupported text history {}", history)),
        }
    }

    fn simple(&mut self, t: &str) -> Res<Value> {
        Ok(match t {
            "IntProperty" => Value::Int(self.c.i32()? as i64),
            "Int8Property" => Value::Int(self.c.u8()? as i8 as i64),
            "Int16Property" | "UInt16Property" => Value::Int(self.c.i16()? as i64),
            "UInt32Property" => Value::Int(self.c.u32()? as i64),
            "Int64Property" | "UInt64Property" => Value::Int(self.c.i64()?),
            "FloatProperty" => Value::Float(self.c.f32()? as f64),
            "DoubleProperty" => Value::Float(self.c.f64()?),
            "NameProperty" | "EnumProperty" => Value::Str(self.name()?),
            "ObjectProperty" | "ClassProperty" | "InterfaceProperty" | "WeakObjectProperty" => {
                let i = self.c.i32()?;
                self.a.obj_ref(i)
            }
            "StrProperty" => Value::Str(self.c.fstring()?),
            "TextProperty" => self.text()?,
            "BoolProperty" | "ByteProperty" => Value::Int(self.c.u8()? as i64),
            "SoftObjectProperty" | "SoftClassProperty" => {
                let path = self.name()?;
                self.c.fstring()?;
                Value::Str(path)
            }
            _ => return Err(format!("unsupported element type {}", t)),
        })
    }

    fn floats(&mut self, n: usize) -> Res<Value> {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(Value::Float(self.c.f32()? as f64));
        }
        Ok(Value::Array(v))
    }

    fn struct_val(&mut self, st: &str) -> Res<Value> {
        match st {
            "Vector" | "Rotator" => self.floats(3),
            "Quat" | "LinearColor" | "Vector4" | "Plane" => self.floats(4),
            "Vector2D" => self.floats(2),
            "Guid" => Ok(Value::Str(self.c.guid_hex()?)),
            "IntPoint" => Ok(Value::Array(vec![Value::Int(self.c.i32()? as i64), Value::Int(self.c.i32()? as i64)])),
            "Color" => Ok(Value::Int(self.c.u32()? as i64)),
            "DateTime" | "Timespan" | "FrameNumber" => Ok(Value::Int(self.c.i64()?)),
            "Box" => {
                self.c.skip(25)?;
                Ok(Value::Null)
            }
            "Box2D" => {
                self.c.skip(17)?;
                Ok(Value::Null)
            }
            "GameplayTagContainer" => {
                let n = self.count()?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(Value::Str(self.name()?));
                }
                Ok(Value::Array(v))
            }
            "SoftObjectPath" | "SoftClassPath" => {
                let path = self.name()?;
                self.c.fstring()?;
                Ok(Value::Str(path))
            }
            _ => self.props(None),
        }
    }
}
