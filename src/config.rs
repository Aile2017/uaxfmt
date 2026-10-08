//! Configuration: built-in defaults, a small TOML subset parser, and TOML
//! output for `-p`.

use crate::encoding::Encoding;

/// Width of East Asian Ambiguous characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AmbiWidth {
    One,
    Two,
    /// Decided by the input encoding (legacy Japanese encodings: 2, Unicode: 1).
    Auto,
}

impl AmbiWidth {
    pub fn parse(s: &str) -> Option<AmbiWidth> {
        match s {
            "1" => Some(AmbiWidth::One),
            "2" => Some(AmbiWidth::Two),
            "auto" => Some(AmbiWidth::Auto),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AmbiWidth::One => "1",
            AmbiWidth::Two => "2",
            AmbiWidth::Auto => "auto",
        }
    }
}

/// Line breaking rule between two ASCII characters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsciiBreak {
    /// Break only after whitespace.
    Space,
    /// Follow UAX #14 (e.g. break after a hyphen).
    Uax14,
}

/// Whether to insert a space when joining lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinSpace {
    /// No space if either side is a wide character.
    M,
    /// No space if both sides are wide characters.
    B,
    /// Always insert a space.
    Always,
}

pub const DEFAULT_NO_LINE_START: &str = concat!(
    "、。，．,.",
    "）〕］｝〉》」』】〙〗〟’”｠⦆»)]}",
    "ヽヾゝゞ々ー",
    "ァィゥェォッャュョヮヵヶぁぃぅぇぉっゃゅょゎゕゖ",
    "‐゠–〜～",
    "？！‼⁇⁈⁉?!・：；:;",
    "…°′″",
);

pub const DEFAULT_NO_LINE_END: &str = "（〔［｛〈《「『【〘〖〝‘“｟«([{";

pub const DEFAULT_LIST_PATTERN: &str = r"^\s*(\d+[.)]|[-*・])\s+";

#[derive(Clone, Debug)]
pub struct Config {
    pub width: usize,
    pub ambiwidth: AmbiWidth,
    pub tabstop: usize,
    pub ascii_break: AsciiBreak,
    pub strict: bool,
    pub no_single_letter_end: bool,
    pub hang: usize,
    pub no_line_start: String,
    pub no_line_end: String,
    pub join_space: JoinSpace,
    pub sentence_space: bool,
    pub trim_trailing: bool,
    pub second_line_indent: bool,
    pub ideographic_space_paragraph: bool,
    pub expandtab: bool,
    pub list_pattern: String,
    pub paragraph_start_pattern: String,
    pub verbatim: bool,
    pub verbatim_start: String,
    pub verbatim_end: String,
    pub verbatim_keep_markers: bool,
    pub comments: Vec<String>,
    /// `None` means auto detection.
    pub encoding: Option<Encoding>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            width: 80,
            ambiwidth: AmbiWidth::Auto,
            tabstop: 8,
            ascii_break: AsciiBreak::Space,
            strict: true,
            no_single_letter_end: false,
            hang: 1,
            no_line_start: DEFAULT_NO_LINE_START.to_string(),
            no_line_end: DEFAULT_NO_LINE_END.to_string(),
            join_space: JoinSpace::M,
            sentence_space: false,
            trim_trailing: true,
            second_line_indent: true,
            ideographic_space_paragraph: true,
            expandtab: false,
            list_pattern: DEFAULT_LIST_PATTERN.to_string(),
            paragraph_start_pattern: String::new(),
            verbatim: false,
            verbatim_start: "^=== Verbatim Begin$".to_string(),
            verbatim_end: "^=== Verbatim End$".to_string(),
            verbatim_keep_markers: false,
            comments: vec!["n:>".to_string(), "b:#".to_string(), "://".to_string()],
            encoding: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    Array(Vec<String>),
}

impl Value {
    fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "integer",
            Value::Bool(_) => "boolean",
            Value::Str(_) => "string",
            Value::Array(_) => "array",
        }
    }
}

impl Config {
    /// Applies the settings in a TOML document on top of `self`.
    pub fn apply_toml(&mut self, src: &str) -> Result<(), String> {
        let entries = parse_toml(src)?;
        let mut seen: Vec<&str> = Vec::new();
        // `_add` / `_remove` keys are applied after the base values.
        let mut edits: Vec<(&str, &Value, usize)> = Vec::new();
        for (key, value, line) in &entries {
            if seen.contains(&key.as_str()) {
                return Err(format!("line {line}: duplicate key '{key}'"));
            }
            seen.push(key);
            if key.ends_with("_add") || key.ends_with("_remove") {
                edits.push((key, value, *line));
                continue;
            }
            self.set(key, value)
                .map_err(|e| format!("line {line}: {e}"))?;
        }
        for suffix in ["_add", "_remove"] {
            for (key, value, line) in edits.iter().filter(|e| e.0.ends_with(suffix)) {
                self.edit_chars(key, value)
                    .map_err(|e| format!("line {line}: {e}"))?;
            }
        }
        Ok(())
    }

    fn set(&mut self, key: &str, value: &Value) -> Result<(), String> {
        match key {
            "width" => self.width = expect_uint(key, value)?,
            "tabstop" => {
                self.tabstop = expect_uint(key, value)?;
                if self.tabstop == 0 {
                    return Err("'tabstop' must be greater than 0".to_string());
                }
            }
            "hang" => self.hang = expect_uint(key, value)?,
            "ambiwidth" => {
                let s = match value {
                    Value::Int(n) => n.to_string(),
                    Value::Str(s) => s.clone(),
                    _ => return Err(type_error(key, "integer or string", value)),
                };
                self.ambiwidth = AmbiWidth::parse(&s)
                    .ok_or_else(|| format!("'ambiwidth' must be 1, 2 or \"auto\": {s}"))?;
            }
            "ascii_break" => {
                self.ascii_break = match expect_str(key, value)? {
                    "space" => AsciiBreak::Space,
                    "uax14" => AsciiBreak::Uax14,
                    s => return Err(format!("'ascii_break' must be \"space\" or \"uax14\": {s}")),
                }
            }
            "join_space" => {
                self.join_space = match expect_str(key, value)? {
                    "M" => JoinSpace::M,
                    "B" => JoinSpace::B,
                    "always" => JoinSpace::Always,
                    s => {
                        return Err(format!(
                            "'join_space' must be \"M\", \"B\" or \"always\": {s}"
                        ));
                    }
                }
            }
            "encoding" => {
                let s = expect_str(key, value)?;
                self.encoding = if s == "auto" {
                    None
                } else {
                    Some(Encoding::from_name(s).ok_or_else(|| format!("unknown encoding: {s}"))?)
                };
            }
            "strict" => self.strict = expect_bool(key, value)?,
            "no_single_letter_end" => self.no_single_letter_end = expect_bool(key, value)?,
            "sentence_space" => self.sentence_space = expect_bool(key, value)?,
            "trim_trailing" => self.trim_trailing = expect_bool(key, value)?,
            "second_line_indent" => self.second_line_indent = expect_bool(key, value)?,
            "ideographic_space_paragraph" => {
                self.ideographic_space_paragraph = expect_bool(key, value)?
            }
            "expandtab" => self.expandtab = expect_bool(key, value)?,
            "no_line_start" => self.no_line_start = expect_str(key, value)?.to_string(),
            "no_line_end" => self.no_line_end = expect_str(key, value)?.to_string(),
            "list_pattern" => self.list_pattern = expect_str(key, value)?.to_string(),
            "paragraph_start_pattern" => {
                self.paragraph_start_pattern = expect_str(key, value)?.to_string()
            }
            "verbatim" => self.verbatim = expect_bool(key, value)?,
            "verbatim_start" => self.verbatim_start = expect_str(key, value)?.to_string(),
            "verbatim_end" => self.verbatim_end = expect_str(key, value)?.to_string(),
            "verbatim_keep_markers" => self.verbatim_keep_markers = expect_bool(key, value)?,
            "comments" => match value {
                Value::Array(a) => self.comments = a.clone(),
                _ => return Err(type_error(key, "array of strings", value)),
            },
            _ => return Err(format!("unknown key '{key}'")),
        }
        Ok(())
    }

    fn edit_chars(&mut self, key: &str, value: &Value) -> Result<(), String> {
        let (base, add) = match key.strip_suffix("_add") {
            Some(b) => (b, true),
            None => (key.strip_suffix("_remove").unwrap_or(key), false),
        };
        let target = match base {
            "no_line_start" => &mut self.no_line_start,
            "no_line_end" => &mut self.no_line_end,
            _ => return Err(format!("unknown key '{key}'")),
        };
        let chars = expect_str(key, value)?;
        if add {
            for c in chars.chars() {
                if !target.contains(c) {
                    target.push(c);
                }
            }
        } else {
            target.retain(|c| !chars.contains(c));
        }
        Ok(())
    }

    /// Renders all settings as a commented TOML document.
    pub fn to_toml(&self) -> String {
        let d = Config::default();
        let mut s = String::from("# uaxfmt config file\n");
        let mut item = |comment: &str, key: &str, value: String, default: String| {
            s.push('\n');
            for line in comment.lines() {
                s.push_str("# ");
                s.push_str(line);
                s.push('\n');
            }
            s.push_str(&format!("# (default: {default})\n{key} = {value}\n"));
        };
        let ambi = |a: AmbiWidth| match a {
            AmbiWidth::Auto => "\"auto\"".to_string(),
            _ => a.name().to_string(),
        };
        let enc = |e: Option<Encoding>| toml_string(e.map_or("auto", |e| e.name()));
        let ascii = |a: AsciiBreak| {
            toml_string(match a {
                AsciiBreak::Space => "space",
                AsciiBreak::Uax14 => "uax14",
            })
        };
        let join = |j: JoinSpace| {
            toml_string(match j {
                JoinSpace::M => "M",
                JoinSpace::B => "B",
                JoinSpace::Always => "always",
            })
        };

        item(
            "Line width in display columns (full-width chars count as 2).\n\
             0 joins lines without wrapping.",
            "width",
            self.width.to_string(),
            d.width.to_string(),
        );
        item(
            "Width of East Asian ambiguous chars (e.g. ○※①): 1, 2 or \"auto\".\n\
             \"auto\" uses 2 for CP932/EUC-JP/ISO-2022-JP input and 1 otherwise.",
            "ambiwidth",
            ambi(self.ambiwidth),
            ambi(d.ambiwidth),
        );
        item(
            "Tab width used to compute display columns.",
            "tabstop",
            self.tabstop.to_string(),
            d.tabstop.to_string(),
        );
        item(
            "Line breaking between two ASCII chars.\n\
             \"space\": break only at spaces. \"uax14\": follow UAX #14 (e.g. after hyphens).",
            "ascii_break",
            ascii(self.ascii_break),
            ascii(d.ascii_break),
        );
        item(
            "Disallow breaks before prolonged sound marks and small kana (UAX #14 CJ class).",
            "strict",
            self.strict.to_string(),
            d.strict.to_string(),
        );
        item(
            "Avoid breaking a line right after a one-letter word (e.g. \"a\", \"I\").",
            "no_single_letter_end",
            self.no_single_letter_end.to_string(),
            d.no_single_letter_end.to_string(),
        );
        item(
            "Max number of chars allowed to hang past the width. 0 disables hanging.",
            "hang",
            self.hang.to_string(),
            d.hang.to_string(),
        );
        item(
            "Chars that must not start a line. They hang past the width (see 'hang').\n\
             Use no_line_start_add / no_line_start_remove to edit the default list.",
            "no_line_start",
            toml_string(&self.no_line_start),
            toml_string(&d.no_line_start),
        );
        item(
            "Chars that must not end a line. They are pushed out to the next line.\n\
             Use no_line_end_add / no_line_end_remove to edit the default list.",
            "no_line_end",
            toml_string(&self.no_line_end),
            toml_string(&d.no_line_end),
        );
        item(
            "Whether to insert a space when joining lines.\n\
             \"M\": not if either side is a wide char. \"B\": not if both sides are wide chars.\n\
             \"always\": always. No space is inserted after 、 or 。 in any case.",
            "join_space",
            join(self.join_space),
            join(d.join_space),
        );
        item(
            "Insert two spaces after '.', '?' and '!' when joining lines.",
            "sentence_space",
            self.sentence_space.to_string(),
            d.sentence_space.to_string(),
        );
        item(
            "Remove trailing whitespace from every output line.",
            "trim_trailing",
            self.trim_trailing.to_string(),
            d.trim_trailing.to_string(),
        );
        item(
            "Indent continuation lines like the second line of the paragraph,\n\
             and start a new paragraph at a line indented deeper than the previous one.",
            "second_line_indent",
            self.second_line_indent.to_string(),
            d.second_line_indent.to_string(),
        );
        item(
            "A line starting with an ideographic space (U+3000) starts a new paragraph.",
            "ideographic_space_paragraph",
            self.ideographic_space_paragraph.to_string(),
            d.ideographic_space_paragraph.to_string(),
        );
        item(
            "Use spaces instead of tabs in indents of continuation lines.",
            "expandtab",
            self.expandtab.to_string(),
            d.expandtab.to_string(),
        );
        item(
            "Regex for list items. A matching line starts a new paragraph and its\n\
             continuation lines are aligned with the item text. Empty disables it.",
            "list_pattern",
            toml_string(&self.list_pattern),
            toml_string(&d.list_pattern),
        );
        let array = |a: &[String]| {
            let items: Vec<String> = a.iter().map(|x| toml_string(x)).collect();
            format!("[{}]", items.join(", "))
        };
        item(
            "Regex matching the start of paragraph text, after indents and comment leaders.\n\
             Starts a new paragraph without list alignment. Empty disables it.\n\
             Example: paragraph_start_pattern = '^【(注意|補足|参考|重要|警告)】'",
            "paragraph_start_pattern",
            toml_string(&self.paragraph_start_pattern),
            toml_string(&d.paragraph_start_pattern),
        );
        item(
            "Preserve lines between verbatim_start and verbatim_end without formatting.",
            "verbatim",
            self.verbatim.to_string(),
            d.verbatim.to_string(),
        );
        item(
            "Regex for the starting marker, matched against the entire raw input line.",
            "verbatim_start",
            toml_string(&self.verbatim_start),
            toml_string(&d.verbatim_start),
        );
        item(
            "Regex for the ending marker. An unclosed range extends to end of input.",
            "verbatim_end",
            toml_string(&self.verbatim_end),
            toml_string(&d.verbatim_end),
        );
        item(
            "Keep verbatim marker lines unchanged; false removes both markers.",
            "verbatim_keep_markers",
            self.verbatim_keep_markers.to_string(),
            d.verbatim_keep_markers.to_string(),
        );
        item(
            "Line leaders such as quote marks, as \"flags:string\".\n\
             Flags: b = needs a blank after it, n = can be nested,\n\
             f = only on the first line (continuation lines get spaces instead).",
            "comments",
            array(&self.comments),
            array(&d.comments),
        );
        item(
            "Input encoding: \"auto\", \"utf-8\", \"utf-16le\", \"utf-16be\", \"cp932\",\n\
             \"euc-jp\" or \"iso-2022-jp\". Output uses the same encoding.",
            "encoding",
            enc(self.encoding),
            enc(d.encoding),
        );
        s
    }
}

fn type_error(key: &str, expected: &str, value: &Value) -> String {
    format!("'{key}' must be {expected}, not {}", value.type_name())
}

fn expect_uint(key: &str, value: &Value) -> Result<usize, String> {
    match value {
        Value::Int(n) if *n >= 0 => Ok(*n as usize),
        Value::Int(_) => Err(format!("'{key}' must not be negative")),
        _ => Err(type_error(key, "an integer", value)),
    }
}

fn expect_bool(key: &str, value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        _ => Err(type_error(key, "a boolean", value)),
    }
}

fn expect_str<'v>(key: &str, value: &'v Value) -> Result<&'v str, String> {
    match value {
        Value::Str(s) => Ok(s),
        _ => Err(type_error(key, "a string", value)),
    }
}

/// Quotes a string for TOML, preferring a literal string when it avoids escapes.
fn toml_string(s: &str) -> String {
    let needs_escape = |c: char| c == '"' || c == '\\' || c.is_control();
    if s.chars().any(needs_escape) && !s.chars().any(|c| c == '\'' || c.is_control()) {
        return format!("'{s}'");
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Parses the subset of TOML used by uaxfmt: top-level `key = value` pairs
/// whose values are integers, booleans, strings or arrays of strings.
fn parse_toml(src: &str) -> Result<Vec<(String, Value, usize)>, String> {
    let mut p = Parser {
        chars: src.chars().collect(),
        pos: 0,
        line: 1,
    };
    let mut entries = Vec::new();
    loop {
        p.skip_ws_comments_newlines();
        let Some(c) = p.peek() else { break };
        let line = p.line;
        if c == '[' {
            return Err(format!("line {line}: tables are not supported"));
        }
        let key = p.parse_key()?;
        p.skip_ws();
        if p.next() != Some('=') {
            return Err(format!("line {line}: expected '=' after key '{key}'"));
        }
        p.skip_ws();
        let value = p.parse_value()?;
        p.skip_ws();
        p.skip_comment();
        match p.peek() {
            None | Some('\n') => {}
            Some('\r') if p.peek_at(1) == Some('\n') => {}
            Some(_) => return Err(format!("line {}: unexpected text after value", p.line)),
        }
        entries.push((key, value, line));
    }
    Ok(entries)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while !matches!(self.peek(), None | Some('\n')) {
                self.pos += 1;
            }
        }
    }

    fn skip_ws_comments_newlines(&mut self) {
        loop {
            self.skip_ws();
            self.skip_comment();
            match self.peek() {
                Some('\n' | '\r') => {
                    self.next();
                }
                _ => break,
            }
        }
    }

    fn err(&self, msg: &str) -> String {
        format!("line {}: {msg}", self.line)
    }

    fn parse_key(&mut self) -> Result<String, String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.err("expected a key"));
        }
        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some('"' | '\'') => Ok(Value::Str(self.parse_string()?)),
            Some('[') => self.parse_array(),
            Some('t' | 'f') => {
                let start = self.pos;
                while matches!(self.peek(), Some(c) if c.is_ascii_alphabetic()) {
                    self.pos += 1;
                }
                match self.chars[start..self.pos]
                    .iter()
                    .collect::<String>()
                    .as_str()
                {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => Err(self.err("invalid value")),
                }
            }
            Some(c) if c.is_ascii_digit() || c == '+' || c == '-' => {
                let start = self.pos;
                self.pos += 1;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '_') {
                    self.pos += 1;
                }
                let text: String = self.chars[start..self.pos]
                    .iter()
                    .filter(|&&c| c != '_')
                    .collect();
                text.parse::<i64>()
                    .map(Value::Int)
                    .map_err(|_| self.err("invalid integer"))
            }
            _ => Err(self.err("expected a value")),
        }
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.next(); // '['
        let mut items = Vec::new();
        loop {
            self.skip_ws_comments_newlines();
            if self.peek() == Some(']') {
                self.next();
                return Ok(Value::Array(items));
            }
            match self.peek() {
                Some('"' | '\'') => items.push(self.parse_string()?),
                None => return Err(self.err("unterminated array")),
                _ => return Err(self.err("arrays may contain only strings")),
            }
            self.skip_ws_comments_newlines();
            match self.next() {
                Some(',') => {}
                Some(']') => return Ok(Value::Array(items)),
                _ => return Err(self.err("expected ',' or ']' in array")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        let quote = self.next().unwrap_or('"');
        if self.peek() == Some(quote) && self.peek_at(1) == Some(quote) {
            return Err(self.err("multi-line strings are not supported"));
        }
        let mut s = String::new();
        loop {
            match self.next() {
                None | Some('\n') => return Err(self.err("unterminated string")),
                Some(c) if c == quote => return Ok(s),
                Some('\\') if quote == '"' => s.push(self.parse_escape()?),
                Some(c) => s.push(c),
            }
        }
    }

    fn parse_escape(&mut self) -> Result<char, String> {
        let c = match self.next() {
            Some('b') => '\u{8}',
            Some('t') => '\t',
            Some('n') => '\n',
            Some('f') => '\u{c}',
            Some('r') => '\r',
            Some('"') => '"',
            Some('\\') => '\\',
            Some(u @ ('u' | 'U')) => {
                let len = if u == 'u' { 4 } else { 8 };
                let hex: String = (0..len).filter_map(|_| self.next()).collect();
                u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| self.err("invalid unicode escape"))?
            }
            _ => return Err(self.err("invalid escape sequence")),
        };
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_values() {
        let mut c = Config::default();
        c.apply_toml(
            "# comment\nwidth = 72 # trailing\r\nhang = 0\nstrict = false\n\
             ambiwidth = 2\njoin_space = \"always\"\nlist_pattern = '^\\d+\\.'\n\
             comments = [\n  \"n:>\", # quote\n  '//',\n]\nencoding = \"cp932\"\n",
        )
        .unwrap();
        assert_eq!(c.width, 72);
        assert_eq!(c.hang, 0);
        assert!(!c.strict);
        assert_eq!(c.ambiwidth, AmbiWidth::Two);
        assert_eq!(c.join_space, JoinSpace::Always);
        assert_eq!(c.list_pattern, r"^\d+\.");
        assert_eq!(c.comments, vec!["n:>", "//"]);
        assert_eq!(c.encoding, Some(Encoding::Cp932));
    }

    #[test]
    fn parses_paragraph_and_verbatim_settings() {
        let mut c = Config::default();
        assert!(c.paragraph_start_pattern.is_empty());
        assert!(!c.verbatim);
        assert!(!c.verbatim_keep_markers);
        c.apply_toml(
            "paragraph_start_pattern = '^【注意】'\nverbatim = true\n\
             verbatim_start = '^BEGIN$'\nverbatim_end = '^END$'\n\
             verbatim_keep_markers = true",
        )
        .unwrap();
        assert_eq!(c.paragraph_start_pattern, "^【注意】");
        assert!(c.verbatim);
        assert_eq!(c.verbatim_start, "^BEGIN$");
        assert_eq!(c.verbatim_end, "^END$");
        assert!(c.verbatim_keep_markers);
        let mut d = Config::default();
        d.apply_toml(&c.to_toml()).unwrap();
        assert_eq!(d.paragraph_start_pattern, c.paragraph_start_pattern);
        assert_eq!(d.verbatim, c.verbatim);
        assert_eq!(d.verbatim_start, c.verbatim_start);
        assert_eq!(d.verbatim_end, c.verbatim_end);
        assert_eq!(d.verbatim_keep_markers, c.verbatim_keep_markers);
        assert!(d.apply_toml("verbatim = 'true'").is_err());
        assert!(d.apply_toml("verbatim_start = false").is_err());
        assert!(d.apply_toml("paragraph_start_pattern = true").is_err());
    }

    #[test]
    fn add_and_remove_chars() {
        let mut c = Config::default();
        c.apply_toml(
            "no_line_end_remove = \"（[\"\nno_line_end = \"「（[\"\nno_line_end_add = \"〔\"",
        )
        .unwrap();
        assert_eq!(c.no_line_end, "「〔");
    }

    #[test]
    fn reports_errors() {
        let mut c = Config::default();
        assert!(
            c.apply_toml("widht = 1")
                .unwrap_err()
                .contains("unknown key")
        );
        assert!(
            c.apply_toml("width = \"1\"")
                .unwrap_err()
                .contains("integer")
        );
        assert!(
            c.apply_toml("width = 1\nwidth = 2")
                .unwrap_err()
                .contains("line 2")
        );
        assert!(c.apply_toml("[table]").is_err());
        assert!(c.apply_toml("width = 1 x").is_err());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut c = Config {
            width: 66,
            ambiwidth: AmbiWidth::One,
            ..Config::default()
        };
        c.no_line_end.push('"');
        let text = c.to_toml();
        let mut d = Config::default();
        d.apply_toml(&text).unwrap();
        assert_eq!(d.width, 66);
        assert_eq!(d.ambiwidth, AmbiWidth::One);
        assert_eq!(d.no_line_end, c.no_line_end);
        assert_eq!(d.list_pattern, c.list_pattern);
        assert_eq!(d.comments, c.comments);
    }
}
