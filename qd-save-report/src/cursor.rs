//! Little-endian byte cursor shared by all binary parsers.

pub type Res<T> = Result<T, String>;

pub struct Cur<'a> {
    pub b: &'a [u8],
    pub p: usize,
}

impl<'a> Cur<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Cur { b, p: 0 }
    }

    pub fn at(b: &'a [u8], p: usize) -> Self {
        Cur { b, p }
    }

    pub fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.p)
    }

    pub fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self.p.checked_add(n).ok_or("length overflow")?;
        if end > self.b.len() {
            return Err(format!("unexpected end of data at offset {} (wanted {} bytes)", self.p, n));
        }
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Res<()> {
        self.take(n).map(|_| ())
    }

    pub fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn i16(&mut self) -> Res<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn i64(&mut self) -> Res<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Res<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn f64(&mut self) -> Res<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// Unreal FString: i32 length (negative = UTF-16), null terminated.
    pub fn fstring(&mut self) -> Res<String> {
        let n = self.i32()?;
        if n == 0 {
            return Ok(String::new());
        }
        let mut s = if n < 0 {
            let len = (n as i64).unsigned_abs() as usize * 2;
            if len > self.remaining() {
                return Err(format!("bad UTF-16 string length {} at {}", n, self.p));
            }
            let raw = self.take(len)?;
            let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        } else {
            let len = n as usize;
            if len > self.remaining() {
                return Err(format!("bad string length {} at {}", n, self.p));
            }
            self.take(len)?.iter().map(|&c| c as char).collect()
        };
        while s.ends_with('\0') {
            s.pop();
        }
        Ok(s)
    }

    pub fn guid_hex(&mut self) -> Res<String> {
        Ok(hex(self.take(16)?))
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}
