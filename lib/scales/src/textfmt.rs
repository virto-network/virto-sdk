use crate::error::Error;
use crate::registry::*;
use crate::value::{sequence_size, Cursor, Value};
use alloc::vec::Vec;
use core::fmt::{self, Write};

/// Maximum nesting depth to prevent stack overflow from recursive types.
const MAX_DEPTH: usize = 64;

/// Format a [`Value`] as a URL-friendly text string.
pub fn to_text(value: &Value<'_>) -> Result<alloc::string::String, Error> {
    let mut out = alloc::string::String::new();
    fmt_value(value, &mut out, MAX_DEPTH)?;
    Ok(out)
}

/// Parse a text-encoded value into SCALE bytes.
pub fn from_text(input: &str, registry: &Registry, ty_id: TypeId) -> Result<Vec<u8>, Error> {
    let mut parser = Parser {
        input,
        pos: 0,
        registry,
        depth: MAX_DEPTH,
    };
    let mut out = Vec::new();
    parser.parse_value(ty_id, &mut out)?;
    if parser.pos != parser.input.len() {
        let trailing = &input[parser.pos..];
        let truncated = if trailing.len() > 40 {
            alloc::format!("{}...", &trailing[..40])
        } else {
            alloc::string::String::from(trailing)
        };
        return Err(Error::BadInput(alloc::format!(
            "unexpected trailing input: '{truncated}'"
        )));
    }
    Ok(out)
}

// ── Display ────────────────────────────────────────────────────────────

impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt_value(self, f, MAX_DEPTH).map_err(|_| fmt::Error)
    }
}

// ── Formatter ──────────────────────────────────────────────────────────

fn fmt_value(value: &Value<'_>, out: &mut impl Write, depth: usize) -> Result<(), Error> {
    let depth = depth.checked_sub(1).ok_or_else(|| {
        Error::BadInput("maximum nesting depth exceeded".into())
    })?;

    let ty = value.ty().ok_or(Error::TypeNotFound(value.ty_id))?;
    let data = value.data;
    let reg = value.registry;

    match ty {
        TypeDef::Bool => {
            write!(out, "{}", value.as_bool().ok_or(Error::Eof)?)?;
        }
        TypeDef::U8 => write!(out, "{}", value.as_u8().ok_or(Error::Eof)?)?,
        TypeDef::U16 => write!(out, "{}", value.as_u16().ok_or(Error::Eof)?)?,
        TypeDef::U32 => write!(out, "{}", value.as_u32().ok_or(Error::Eof)?)?,
        TypeDef::U64 => write!(out, "{}", value.as_u64().ok_or(Error::Eof)?)?,
        TypeDef::U128 => write!(out, "{}", value.as_u128().ok_or(Error::Eof)?)?,
        TypeDef::I8 => write!(out, "{}", value.as_i8().ok_or(Error::Eof)?)?,
        TypeDef::I16 => write!(out, "{}", value.as_i16().ok_or(Error::Eof)?)?,
        TypeDef::I32 => write!(out, "{}", value.as_i32().ok_or(Error::Eof)?)?,
        TypeDef::I64 => write!(out, "{}", value.as_i64().ok_or(Error::Eof)?)?,
        TypeDef::I128 => write!(out, "{}", value.as_i128().ok_or(Error::Eof)?)?,
        TypeDef::Char => {
            let c = value.as_char().ok_or(Error::Eof)?;
            write!(out, "{c}")?;
        }
        TypeDef::Str => {
            let s = value.as_str().ok_or(Error::Eof)?;
            out.write_char('\'')?;
            for c in s.chars() {
                if c == '\'' {
                    out.write_str("''")?;
                } else {
                    out.write_char(c)?;
                }
            }
            out.write_char('\'')?;
        }
        TypeDef::Bytes => {
            let (len, prefix) = sequence_size(data)?;
            let bytes = data.get(prefix..prefix + len).ok_or(Error::Eof)?;
            write_hex(bytes, out)?;
        }
        TypeDef::Compact(_) => {
            let (val, _) = sequence_size(data)?;
            write!(out, "{val}")?;
        }
        TypeDef::Tuple(tys) => {
            out.write_char('(')?;
            let mut cursor = Cursor::new(data, reg);
            for (i, ty_id) in tys.iter().enumerate() {
                if i > 0 {
                    out.write_char(';')?;
                }
                fmt_value(&cursor.next_value(*ty_id)?, out, depth)?;
            }
            out.write_char(')')?;
        }
        TypeDef::StructUnit => {}
        TypeDef::StructNewType(inner) => {
            fmt_value(&Value::new(data, *inner, reg), out, depth)?;
        }
        TypeDef::StructTuple(tys) => {
            out.write_char('(')?;
            let mut cursor = Cursor::new(data, reg);
            for (i, ty_id) in tys.iter().enumerate() {
                if i > 0 {
                    out.write_char(';')?;
                }
                fmt_value(&cursor.next_value(*ty_id)?, out, depth)?;
            }
            out.write_char(')')?;
        }
        TypeDef::Struct(fields) => {
            out.write_char('(')?;
            let mut cursor = Cursor::new(data, reg);
            for (i, f) in fields.iter().enumerate() {
                if i > 0 {
                    out.write_char(';')?;
                }
                out.write_str(&f.name)?;
                out.write_char(':')?;
                fmt_value(&cursor.next_value(f.ty)?, out, depth)?;
            }
            out.write_char(')')?;
        }
        TypeDef::Variant(vdef) => {
            let idx = *data.first().ok_or(Error::Eof)?;
            let var = vdef.variant(idx)?;
            out.write_str(&vdef.name)?;
            out.write_str("::")?;
            out.write_str(&var.name)?;
            let inner = &data[1..];
            match &var.fields {
                Fields::Unit => {}
                Fields::NewType(ty_id) => {
                    out.write_char('(')?;
                    fmt_value(&Value::new(inner, *ty_id, reg), out, depth)?;
                    out.write_char(')')?;
                }
                Fields::Tuple(tys) => {
                    out.write_char('(')?;
                    let mut cursor = Cursor::new(inner, reg);
                    for (i, ty_id) in tys.iter().enumerate() {
                        if i > 0 {
                            out.write_char(';')?;
                        }
                        fmt_value(&cursor.next_value(*ty_id)?, out, depth)?;
                    }
                    out.write_char(')')?;
                }
                Fields::Struct(fields) => {
                    out.write_char('(')?;
                    let mut cursor = Cursor::new(inner, reg);
                    for (i, f) in fields.iter().enumerate() {
                        if i > 0 {
                            out.write_char(';')?;
                        }
                        out.write_str(&f.name)?;
                        out.write_char(':')?;
                        fmt_value(&cursor.next_value(f.ty)?, out, depth)?;
                    }
                    out.write_char(')')?;
                }
            }
        }
        TypeDef::Sequence(inner_ty) => {
            let (len, prefix) = sequence_size(data)?;
            out.write_str("..")?;
            let mut cursor = Cursor::new(&data[prefix..], reg);
            for i in 0..len {
                if i > 0 {
                    out.write_char(';')?;
                }
                fmt_value(&cursor.next_value(*inner_ty)?, out, depth)?;
            }
            out.write_char('.')?;
        }
        TypeDef::Array(inner_ty, len) => {
            out.write_str("..")?;
            let mut cursor = Cursor::new(data, reg);
            for i in 0..*len {
                if i > 0 {
                    out.write_char(';')?;
                }
                fmt_value(&cursor.next_value(*inner_ty)?, out, depth)?;
            }
            out.write_char('.')?;
        }
        TypeDef::Map(ty_k, ty_v) => {
            let (len, prefix) = sequence_size(data)?;
            out.write_str("..")?;
            let mut cursor = Cursor::new(&data[prefix..], reg);
            for i in 0..len {
                if i > 0 {
                    out.write_char(';')?;
                }
                out.write_char('(')?;
                fmt_value(&cursor.next_value(*ty_k)?, out, depth)?;
                out.write_char(';')?;
                fmt_value(&cursor.next_value(*ty_v)?, out, depth)?;
                out.write_char(')')?;
            }
            out.write_char('.')?;
        }
        TypeDef::BitSequence(_, _) => {
            let (bit_len, prefix) = sequence_size(data)?;
            let byte_len = bit_len.div_ceil(8);
            let bytes = data.get(prefix..prefix + byte_len).ok_or(Error::Eof)?;
            write_hex(bytes, out)?;
        }
    }
    Ok(())
}

/// Write bytes as `0x`-prefixed lowercase hex directly to a `fmt::Write`,
/// without allocating an intermediate string.
fn write_hex(bytes: &[u8], out: &mut impl Write) -> fmt::Result {
    out.write_str("0x")?;
    for &b in bytes {
        write!(out, "{:02x}", b)?;
    }
    Ok(())
}

/// Returns true if `c` can immediately follow a bare keyword (true/false)
/// or identifier without being part of it.
fn is_delimiter(c: char) -> bool {
    matches!(c, ';' | ')' | '.' | ':' | '(')
}

// ── Parser ─────────────────────────────────────────────────────────────

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    registry: &'a Registry,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn advance(&mut self, n: usize) {
        self.pos += n;
    }

    fn expect(&mut self, c: char) -> Result<(), Error> {
        if self.peek() == Some(c) {
            self.advance(c.len_utf8());
            Ok(())
        } else {
            Err(Error::BadInput(alloc::format!(
                "expected '{}' at position {}",
                c, self.pos
            )))
        }
    }

    fn consume(&mut self, s: &str) -> bool {
        if self.remaining().starts_with(s) {
            self.advance(s.len());
            true
        } else {
            false
        }
    }

    fn take_while(&mut self, pred: impl Fn(char) -> bool) -> &'a str {
        let start = self.pos;
        while self.peek().is_some_and(&pred) {
            self.advance(self.peek().unwrap().len_utf8());
        }
        &self.input[start..self.pos]
    }

    /// A single `.` not followed by another `.` closes a sequence.
    fn at_seq_close(&self) -> bool {
        let r = self.remaining();
        r.starts_with('.') && !r.starts_with("..")
    }

    /// Check that the next char (if any) is a structural delimiter,
    /// ensuring a keyword like `true` wasn't parsed from `truer`.
    fn expect_delimiter(&self) -> Result<(), Error> {
        match self.peek() {
            None => Ok(()),
            Some(c) if is_delimiter(c) => Ok(()),
            Some(c) => Err(Error::BadInput(alloc::format!(
                "unexpected character '{c}' at position {}",
                self.pos
            ))),
        }
    }

    fn parse_uint(&mut self) -> Result<u128, Error> {
        let s = self.take_while(|c| c.is_ascii_digit());
        if s.is_empty() {
            return Err(Error::BadInput(alloc::format!(
                "expected number at position {}",
                self.pos
            )));
        }
        s.parse::<u128>()
            .map_err(|e| Error::BadInput(alloc::string::ToString::to_string(&e)))
    }

    fn parse_uint_checked<T: TryFrom<u128>>(&mut self) -> Result<T, Error> {
        let n = self.parse_uint()?;
        T::try_from(n).map_err(|_| Error::BadInput(alloc::format!("{n} out of range")))
    }

    fn parse_int(&mut self) -> Result<i128, Error> {
        let neg = self.consume("-");
        let n = self.parse_uint()?;
        let n = n as i128;
        Ok(if neg { -n } else { n })
    }

    fn parse_int_checked<T: TryFrom<i128>>(&mut self) -> Result<T, Error> {
        let n = self.parse_int()?;
        T::try_from(n).map_err(|_| Error::BadInput(alloc::format!("{n} out of range")))
    }

    fn parse_hex_bytes(&mut self) -> Result<Vec<u8>, Error> {
        if !self.consume("0x") {
            return Err(Error::BadInput("expected '0x' prefix".into()));
        }
        let hex_str = self.take_while(|c| c.is_ascii_hexdigit());
        crate::decode_hex(hex_str)
    }

    fn parse_value(&mut self, ty_id: TypeId, out: &mut Vec<u8>) -> Result<(), Error> {
        self.depth = self.depth.checked_sub(1).ok_or_else(|| {
            Error::BadInput("maximum nesting depth exceeded".into())
        })?;

        let result = self.parse_value_inner(ty_id, out);

        self.depth += 1;
        result
    }

    fn parse_value_inner(&mut self, ty_id: TypeId, out: &mut Vec<u8>) -> Result<(), Error> {
        let reg = self.registry;
        let ty = reg.resolve(ty_id).ok_or(Error::TypeNotFound(ty_id))?;

        match ty {
            TypeDef::Bool => {
                if self.consume("true") {
                    self.expect_delimiter()?;
                    out.push(1);
                } else if self.consume("false") {
                    self.expect_delimiter()?;
                    out.push(0);
                } else {
                    return Err(Error::BadInput("expected 'true' or 'false'".into()));
                }
            }
            TypeDef::U8 => {
                let n: u8 = self.parse_uint_checked()?;
                out.push(n);
            }
            TypeDef::U16 => {
                let n: u16 = self.parse_uint_checked()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::U32 => {
                let n: u32 = self.parse_uint_checked()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::U64 => {
                let n: u64 = self.parse_uint_checked()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::U128 => {
                let n = self.parse_uint()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::I8 => {
                let n: i8 = self.parse_int_checked()?;
                out.push(n as u8);
            }
            TypeDef::I16 => {
                let n: i16 = self.parse_int_checked()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::I32 => {
                let n: i32 = self.parse_int_checked()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::I64 => {
                let n: i64 = self.parse_int_checked()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::I128 => {
                let n = self.parse_int()?;
                out.extend_from_slice(&n.to_le_bytes());
            }
            TypeDef::Char => {
                let c = self.peek().ok_or(Error::Eof)?;
                self.advance(c.len_utf8());
                out.extend_from_slice(&(c as u32).to_le_bytes());
            }
            TypeDef::Str => {
                self.expect('\'')?;
                // Parse string content, handling '' escape for literal single quotes
                let mut s = Vec::new();
                loop {
                    match self.peek() {
                        None => return Err(Error::BadInput("unterminated string".into())),
                        Some('\'') => {
                            self.advance(1);
                            // '' is an escaped single quote, lone ' closes the string
                            if self.peek() == Some('\'') {
                                self.advance(1);
                                s.push(b'\'');
                            } else {
                                break;
                            }
                        }
                        Some(c) => {
                            self.advance(c.len_utf8());
                            let mut buf = [0u8; 4];
                            s.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        }
                    }
                }
                crate::compact_encode(s.len() as u128, &mut *out);
                out.extend_from_slice(&s);
            }
            TypeDef::Bytes => {
                let bytes = self.parse_hex_bytes()?;
                crate::compact_encode(bytes.len() as u128, &mut *out);
                out.extend_from_slice(&bytes);
            }
            TypeDef::Compact(_) => {
                let n = self.parse_uint()?;
                crate::compact_encode(n, out);
            }
            TypeDef::Tuple(tys) => {
                self.expect('(')?;
                for (i, ty_id) in tys.iter().enumerate() {
                    if i > 0 {
                        self.expect(';')?;
                    }
                    self.parse_value(*ty_id, out)?;
                }
                self.expect(')')?;
            }
            TypeDef::StructUnit => {}
            TypeDef::StructNewType(inner) => {
                let inner = *inner;
                self.parse_value(inner, out)?;
            }
            TypeDef::StructTuple(tys) => {
                self.expect('(')?;
                for (i, ty_id) in tys.iter().enumerate() {
                    if i > 0 {
                        self.expect(';')?;
                    }
                    self.parse_value(*ty_id, out)?;
                }
                self.expect(')')?;
            }
            TypeDef::Struct(fields) => {
                self.expect('(')?;
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.expect(';')?;
                    }
                    if !self.consume(&f.name) {
                        return Err(Error::BadInput(alloc::format!(
                            "expected field '{}'",
                            f.name
                        )));
                    }
                    self.expect(':')?;
                    self.parse_value(f.ty, out)?;
                }
                self.expect(')')?;
            }
            TypeDef::Variant(vdef) => {
                if !self.remaining().starts_with(vdef.name.as_str()) {
                    return Err(Error::BadInput(alloc::format!(
                        "expected enum '{}'",
                        vdef.name
                    )));
                }
                self.advance(vdef.name.len());
                self.expect(':')?;
                self.expect(':')?;
                let var_name = self.take_while(|c| c.is_alphanumeric() || c == '_');
                let var = vdef
                    .variants
                    .iter()
                    .find(|v| v.name == var_name)
                    .ok_or_else(|| {
                        Error::BadInput(alloc::format!("unknown variant '{var_name}'"))
                    })?;
                out.push(var.index);
                match &var.fields {
                    Fields::Unit => {}
                    Fields::NewType(ty_id) => {
                        self.expect('(')?;
                        self.parse_value(*ty_id, out)?;
                        self.expect(')')?;
                    }
                    Fields::Tuple(tys) => {
                        self.expect('(')?;
                        for (i, ty_id) in tys.iter().enumerate() {
                            if i > 0 {
                                self.expect(';')?;
                            }
                            self.parse_value(*ty_id, out)?;
                        }
                        self.expect(')')?;
                    }
                    Fields::Struct(fields) => {
                        self.expect('(')?;
                        for (i, f) in fields.iter().enumerate() {
                            if i > 0 {
                                self.expect(';')?;
                            }
                            if !self.consume(&f.name) {
                                return Err(Error::BadInput(alloc::format!(
                                    "expected field '{}'",
                                    f.name
                                )));
                            }
                            self.expect(':')?;
                            self.parse_value(f.ty, out)?;
                        }
                        self.expect(')')?;
                    }
                }
            }
            TypeDef::Sequence(inner_ty) => {
                let inner_ty = *inner_ty;
                if !self.consume("..") {
                    return Err(Error::BadInput("expected '..' for sequence".into()));
                }
                let mut items = Vec::new();
                let mut count = 0usize;
                if !self.at_seq_close() {
                    loop {
                        self.parse_value(inner_ty, &mut items)?;
                        count += 1;
                        if !self.consume(";") || self.at_seq_close() {
                            break;
                        }
                    }
                }
                self.expect('.')?;
                crate::compact_encode(count as u128, &mut *out);
                out.extend_from_slice(&items);
            }
            TypeDef::Array(inner_ty, len) => {
                let (inner_ty, len) = (*inner_ty, *len);
                if !self.consume("..") {
                    return Err(Error::BadInput("expected '..' for array".into()));
                }
                for i in 0..len {
                    if i > 0 {
                        self.expect(';')?;
                    }
                    self.parse_value(inner_ty, out)?;
                }
                self.expect('.')?;
            }
            TypeDef::Map(ty_k, ty_v) => {
                let (ty_k, ty_v) = (*ty_k, *ty_v);
                if !self.consume("..") {
                    return Err(Error::BadInput("expected '..' for map".into()));
                }
                let mut items = Vec::new();
                let mut count = 0usize;
                if !self.at_seq_close() {
                    loop {
                        self.expect('(')?;
                        self.parse_value(ty_k, &mut items)?;
                        self.expect(';')?;
                        self.parse_value(ty_v, &mut items)?;
                        self.expect(')')?;
                        count += 1;
                        if !self.consume(";") || self.at_seq_close() {
                            break;
                        }
                    }
                }
                self.expect('.')?;
                crate::compact_encode(count as u128, &mut *out);
                out.extend_from_slice(&items);
            }
            TypeDef::BitSequence(_, _) => {
                let bytes = self.parse_hex_bytes()?;
                let bit_len = bytes.len() * 8;
                crate::compact_encode(bit_len as u128, &mut *out);
                out.extend_from_slice(&bytes);
            }
        }
        Ok(())
    }
}
