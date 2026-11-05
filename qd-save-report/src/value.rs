//! Generic property value tree produced by both the save (GVAS) parser and the
//! cooked asset (.uasset) parser.

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Struct(Vec<(String, Value)>),
}

static EMPTY_ARR: [Value; 0] = [];
static EMPTY_MAP: [(Value, Value); 0] = [];

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Struct(fields) => fields.iter().find(|(n, _)| n == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// First field whose name starts with `prefix` (Blueprint struct members
    /// carry GUID suffixes like `AmountNeeded_7_E9D3...`).
    pub fn get_prefix(&self, prefix: &str) -> Option<&Value> {
        match self {
            Value::Struct(fields) => fields.iter().find(|(n, _)| n.starts_with(prefix)).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Value] {
        match self {
            Value::Array(a) => a,
            _ => &EMPTY_ARR,
        }
    }

    pub fn as_map(&self) -> &[(Value, Value)] {
        match self {
            Value::Map(m) => m,
            _ => &EMPTY_MAP,
        }
    }

    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        let a = self.as_array();
        if a.len() == 3 {
            Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
        } else {
            None
        }
    }
}
