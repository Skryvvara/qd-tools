//! Parser for Unreal Engine 4 `.sav` files (GVAS / USaveGame tagged properties).
//!
//! In save files every FName is written as a plain FString, and struct types
//! without native serializers are written as nested tagged property lists.

use crate::cursor::{Cur, Res};
use crate::value::Value;

pub struct SaveFile {
    #[allow(dead_code)]
    pub class: String,
    pub root: Value,
}

pub fn parse(b: &[u8]) -> Res<SaveFile> {
    let mut c = Cur::new(b);
    if c.take(4)? != b"GVAS" {
        return Err("not a GVAS save file".into());
    }
    c.i32()?; // save game file version
    c.i32()?; // package (UE4) version
    c.skip(6)?; // engine major/minor/patch
    c.u32()?; // changelist
    c.fstring()?; // branch
    c.i32()?; // custom version format
    let n = c.i32()?;
    if !(0..=10_000).contains(&n) {
        return Err("bad custom version count".into());
    }
    c.skip(n as usize * 20)?;
    let class = c.fstring()?;
    let root = props(&mut c)?;
    Ok(SaveFile { class, root })
}

fn props(c: &mut Cur) -> Res<Value> {
    let mut out = Vec::new();
    while c.remaining() > 0 {
        let name = c.fstring()?;
        if name.is_empty() || name == "None" {
            break;
        }
        let t = c.fstring()?;
        let size = c.i32()?;
        let _index = c.i32()?;
        if size < 0 {
            return Err(format!("negative size for property {}", name));
        }
        let v = prop(c, &t, size as usize)?;
        out.push((name, v));
    }
    Ok(Value::Struct(out))
}

fn end_of(c: &Cur, size: usize) -> Res<usize> {
    let end = c.p + size;
    if end > c.b.len() {
        Err(format!("property overruns file ({} > {})", end, c.b.len()))
    } else {
        Ok(end)
    }
}

fn prop(c: &mut Cur, t: &str, size: usize) -> Res<Value> {
    match t {
        "BoolProperty" => {
            let v = c.u8()?;
            c.u8()?;
            Ok(Value::Bool(v != 0))
        }
        "StructProperty" => {
            let st = c.fstring()?;
            c.skip(16)?;
            c.u8()?;
            let end = end_of(c, size)?;
            let v = struct_val(c, &st).unwrap_or(Value::Null);
            c.p = end;
            Ok(v)
        }
        "ByteProperty" => {
            let en = c.fstring()?;
            c.u8()?;
            let end = end_of(c, size)?;
            let v = if en == "None" {
                if size == 1 { Value::Int(c.u8()? as i64) } else { Value::Null }
            } else {
                Value::Str(c.fstring()?)
            };
            c.p = end;
            Ok(v)
        }
        "EnumProperty" => {
            c.fstring()?;
            c.u8()?;
            let end = end_of(c, size)?;
            let v = c.fstring().map(Value::Str).unwrap_or(Value::Null);
            c.p = end;
            Ok(v)
        }
        "ArrayProperty" => {
            let it = c.fstring()?;
            c.u8()?;
            let end = end_of(c, size)?;
            let v = array_val(c, &it, size).unwrap_or(Value::Null);
            c.p = end;
            Ok(v)
        }
        "SetProperty" => {
            let it = c.fstring()?;
            c.u8()?;
            let end = end_of(c, size)?;
            let v = set_val(c, &it).unwrap_or(Value::Null);
            c.p = end;
            Ok(v)
        }
        "MapProperty" => {
            let kt = c.fstring()?;
            let vt = c.fstring()?;
            c.u8()?;
            let end = end_of(c, size)?;
            let v = map_val(c, &kt, &vt).unwrap_or(Value::Null);
            c.p = end;
            Ok(v)
        }
        _ => {
            c.u8()?;
            let end = end_of(c, size)?;
            let v = simple(c, t).unwrap_or(Value::Null);
            c.p = end;
            Ok(v)
        }
    }
}

fn count(c: &mut Cur) -> Res<usize> {
    let n = c.i32()?;
    if n < 0 || n as usize > c.remaining() {
        return Err(format!("implausible element count {}", n));
    }
    Ok(n as usize)
}

fn array_val(c: &mut Cur, it: &str, size: usize) -> Res<Value> {
    let n = count(c)?;
    let mut v = Vec::with_capacity(n);
    if it == "StructProperty" {
        c.fstring()?; // property name
        c.fstring()?; // "StructProperty"
        c.i64()?; // size + index
        let st = c.fstring()?;
        c.skip(17)?; // guid + has-guid
        for _ in 0..n {
            v.push(struct_val(c, &st)?);
        }
    } else if it == "ByteProperty" && size == n + 4 {
        for &b in c.take(n)? {
            v.push(Value::Int(b as i64));
        }
    } else {
        for _ in 0..n {
            v.push(simple(c, it)?);
        }
    }
    Ok(Value::Array(v))
}

fn set_val(c: &mut Cur, it: &str) -> Res<Value> {
    let removed = c.i32()?;
    if removed != 0 {
        return Err("sets with removed entries are not supported".into());
    }
    let n = count(c)?;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(if it == "StructProperty" { Value::Str(c.guid_hex()?) } else { simple(c, it)? });
    }
    Ok(Value::Array(v))
}

fn map_val(c: &mut Cur, kt: &str, vt: &str) -> Res<Value> {
    let removed = c.i32()?;
    if removed != 0 {
        return Err("maps with removed entries are not supported".into());
    }
    let n = count(c)?;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        // Struct keys carry no type information; in this game they are GUIDs.
        let k = if kt == "StructProperty" { Value::Str(c.guid_hex()?) } else { simple(c, kt)? };
        let val = if vt == "StructProperty" { props(c)? } else { simple(c, vt)? };
        v.push((k, val));
    }
    Ok(Value::Map(v))
}

fn simple(c: &mut Cur, t: &str) -> Res<Value> {
    Ok(match t {
        "IntProperty" => Value::Int(c.i32()? as i64),
        "Int8Property" => Value::Int(c.u8()? as i8 as i64),
        "Int16Property" | "UInt16Property" => Value::Int(c.i16()? as i64),
        "UInt32Property" => Value::Int(c.u32()? as i64),
        "Int64Property" | "UInt64Property" => Value::Int(c.i64()?),
        "FloatProperty" => Value::Float(c.f32()? as f64),
        "DoubleProperty" => Value::Float(c.f64()?),
        "StrProperty" | "NameProperty" | "ObjectProperty" | "EnumProperty" | "ClassProperty" => Value::Str(c.fstring()?),
        "SoftObjectProperty" | "SoftClassProperty" => {
            let path = c.fstring()?;
            c.fstring()?; // sub path
            Value::Str(path)
        }
        "BoolProperty" | "ByteProperty" => Value::Int(c.u8()? as i64),
        _ => return Err(format!("unsupported element type {}", t)),
    })
}

fn floats(c: &mut Cur, n: usize) -> Res<Value> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(Value::Float(c.f32()? as f64));
    }
    Ok(Value::Array(v))
}

fn struct_val(c: &mut Cur, st: &str) -> Res<Value> {
    match st {
        "Guid" => Ok(Value::Str(c.guid_hex()?)),
        "Vector" | "Rotator" => floats(c, 3),
        "Quat" | "LinearColor" | "Vector4" | "Plane" => floats(c, 4),
        "Vector2D" => floats(c, 2),
        "IntPoint" => Ok(Value::Array(vec![Value::Int(c.i32()? as i64), Value::Int(c.i32()? as i64)])),
        "DateTime" | "Timespan" => Ok(Value::Int(c.i64()?)),
        "Color" => Ok(Value::Int(c.u32()? as i64)),
        _ => props(c),
    }
}
