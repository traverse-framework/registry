//! Minimal JSON value type, parser, and writer for `no_std` + `alloc`.
//!
//! Not a general-purpose/adversarial-input JSON library: it targets the
//! well-formed, contract-validated payloads a Traverse capability actually
//! receives (its own declared `inputs.schema`) and produces (its own
//! `outputs.schema`). Depth is unbounded via recursion, which is acceptable
//! for the small, flat-ish shapes these capabilities use.

use alloc::string::String;
use alloc::vec::Vec;
use core::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Field lookup on an `Object`; `None` for any other variant or missing key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn string_array(&self) -> Vec<String> {
        self.as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Builder helper: an object from `(&str, Value)` pairs, in declaration order.
pub fn object(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(k, v)| (String::from(k), v))
            .collect(),
    )
}

pub fn array_of_strings(items: &[String]) -> Value {
    Value::Array(items.iter().cloned().map(Value::String).collect())
}

// ---------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------

struct Parser<'a> {
    chars: Chars<'a>,
    peeked: Option<char>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Parser {
            chars: input.chars(),
            peeked: None,
        }
    }

    fn peek(&mut self) -> Option<char> {
        if self.peeked.is_none() {
            self.peeked = self.chars.next();
        }
        self.peeked
    }

    fn bump(&mut self) -> Option<char> {
        if let Some(c) = self.peeked.take() {
            return Some(c);
        }
        self.chars.next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), ()> {
        match self.bump() {
            Some(c) if c == expected => Ok(()),
            _ => Err(()),
        }
    }

    fn parse_value(&mut self) -> Result<Value, ()> {
        self.skip_whitespace();
        match self.peek().ok_or(())? {
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            '"' => self.parse_string().map(Value::String),
            't' | 'f' => self.parse_bool(),
            'n' => self.parse_null(),
            '-' | '0'..='9' => self.parse_number(),
            _ => Err(()),
        }
    }

    fn parse_object(&mut self) -> Result<Value, ()> {
        self.expect('{')?;
        let mut fields = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Value::Object(fields));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(':')?;
            let value = self.parse_value()?;
            fields.push((key, value));
            self.skip_whitespace();
            match self.bump() {
                Some(',') => continue,
                Some('}') => break,
                _ => return Err(()),
            }
        }
        Ok(Value::Object(fields))
    }

    fn parse_array(&mut self) -> Result<Value, ()> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Value::Array(items));
        }
        loop {
            let value = self.parse_value()?;
            items.push(value);
            self.skip_whitespace();
            match self.bump() {
                Some(',') => continue,
                Some(']') => break,
                _ => return Err(()),
            }
        }
        Ok(Value::Array(items))
    }

    fn parse_string(&mut self) -> Result<String, ()> {
        self.skip_whitespace();
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.bump().ok_or(())? {
                '"' => break,
                '\\' => match self.bump().ok_or(())? {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let mut code: u32 = 0;
                        for _ in 0..4 {
                            let c = self.bump().ok_or(())?;
                            let digit = c.to_digit(16).ok_or(())?;
                            code = code * 16 + digit;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    _ => return Err(()),
                },
                c => out.push(c),
            }
        }
        Ok(out)
    }

    /// Dispatches on the peeked first character rather than trying "true"
    /// then unconditionally falling through to "false": `consume_literal`
    /// has no backtracking, so a failed "true" attempt against `false`
    /// input still consumes the 'f', corrupting the stream before the
    /// "false" attempt even starts. Peeking first avoids ever attempting
    /// the wrong literal.
    fn parse_bool(&mut self) -> Result<Value, ()> {
        match self.peek() {
            Some('t') if self.consume_literal("true") => Ok(Value::Bool(true)),
            Some('f') if self.consume_literal("false") => Ok(Value::Bool(false)),
            _ => Err(()),
        }
    }

    fn parse_null(&mut self) -> Result<Value, ()> {
        if self.consume_literal("null") {
            Ok(Value::Null)
        } else {
            Err(())
        }
    }

    fn consume_literal(&mut self, literal: &str) -> bool {
        for expected in literal.chars() {
            if self.bump() != Some(expected) {
                return false;
            }
        }
        true
    }

    fn parse_number(&mut self) -> Result<Value, ()> {
        let mut buf = String::new();
        if self.peek() == Some('-') {
            buf.push(self.bump().unwrap());
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                buf.push(self.bump().unwrap());
            } else {
                break;
            }
        }
        if self.peek() == Some('.') {
            buf.push(self.bump().unwrap());
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    buf.push(self.bump().unwrap());
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            buf.push(self.bump().unwrap());
            if matches!(self.peek(), Some('+') | Some('-')) {
                buf.push(self.bump().unwrap());
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    buf.push(self.bump().unwrap());
                } else {
                    break;
                }
            }
        }
        buf.parse::<f64>().map(Value::Number).map_err(|_| ())
    }
}

pub fn parse(input: &str) -> Result<Value, ()> {
    let mut parser = Parser::new(input);
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    Ok(value)
}

// ---------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------

pub fn write(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => write_number(*n, out),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(fields) => {
            out.push('{');
            for (i, (key, val)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_value(val, out);
            }
            out.push('}');
        }
    }
}

/// Deliberately avoids `f64::round`/`abs`/`fract`: on `wasm32-unknown-unknown`
/// those can pull in libm symbols that aren't linked in this no_std binary.
/// Everything here is casts and integer arithmetic only.
fn write_number(n: f64, out: &mut String) {
    let as_int = n as i64;
    let is_whole = (as_int as f64) == n && n > -1.0e15 && n < 1.0e15;
    if is_whole {
        write_integer(as_int, out);
        return;
    }
    let negative = n < 0.0;
    let magnitude = if negative { -n } else { n };
    let scaled = (magnitude * 1_000_000.0) as i64; // truncating cast, not .round()
    let whole = scaled / 1_000_000;
    let frac = scaled % 1_000_000;
    if negative {
        out.push('-');
    }
    write_integer(whole, out);
    out.push('.');
    let frac_str_start = out.len();
    write_integer(frac, out);
    let digits_written = out.len() - frac_str_start;
    if digits_written < 6 {
        let padding = 6 - digits_written;
        for _ in 0..padding {
            out.insert(frac_str_start, '0');
        }
    }
}

fn write_integer(n: i64, out: &mut String) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut n = n;
    if n < 0 {
        out.push('-');
        n = -n;
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push((n % 10) as u8);
        n /= 10;
    }
    for d in digits.iter().rev() {
        out.push((b'0' + d) as char);
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u00");
                let code = c as u32;
                let hex = b"0123456789abcdef";
                out.push(hex[(code as usize >> 4) & 0xf] as char);
                out.push(hex[code as usize & 0xf] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    #[test]
    fn round_trips_a_simple_object() {
        let parsed = parse(r#"{"a": "hello", "b": 3, "c": [1,2,3], "d": true, "e": null}"#)
            .expect("should parse");
        let written = write(&parsed);
        let reparsed = parse(&written).expect("should reparse");
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn parses_string_arrays() {
        let parsed = parse(r#"{"tags": ["a", "b", "c"]}"#).expect("should parse");
        let tags = parsed.get("tags").unwrap().string_array();
        assert_eq!(tags, alloc::vec![
            String::from("a"),
            String::from("b"),
            String::from("c")
        ]);
    }

    #[test]
    fn handles_escaped_characters() {
        let parsed = parse(r#"{"note": "line1\nline2 \"quoted\""}"#).expect("should parse");
        assert_eq!(
            parsed.get("note").unwrap().as_str().unwrap(),
            "line1\nline2 \"quoted\""
        );
    }

    #[test]
    fn writes_valid_json_for_nested_structures() {
        let value = object(alloc::vec![
            ("items", Value::Array(alloc::vec![
                object(alloc::vec![("task", Value::String(String::from("call bob")))]),
            ])),
        ]);
        let written = write(&value);
        let reparsed = parse(&written).expect("round trip must reparse");
        assert_eq!(value, reparsed);
    }

    /// Regression test for a real bug: `parse_bool` used to try "true" then
    /// unconditionally fall through to "false" on any mismatch, but
    /// `consume_literal` has no backtracking -- a failed "true" attempt
    /// against `false` input still consumed the 'f', corrupting the stream
    /// so the "false" attempt failed too. This shipped silently in this
    /// crate's earlier published capabilities since none of them happened
    /// to have a boolean *input* field; caught only once one did.
    #[test]
    fn parses_false_correctly_alone_and_alongside_other_fields() {
        assert_eq!(parse(r#"{"a": false}"#).unwrap(), object(alloc::vec![("a", Value::Bool(false))]));
        assert_eq!(
            parse(r#"{"email": "alice+x@example.com", "allow_plus_addressing": false}"#).unwrap(),
            object(alloc::vec![
                ("email", Value::String(String::from("alice+x@example.com"))),
                ("allow_plus_addressing", Value::Bool(false)),
            ])
        );
    }
}
