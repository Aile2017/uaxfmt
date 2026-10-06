//! Paragraph formatting: splitting into paragraphs, joining lines, and
//! wrapping with UAX #14 line breaking and Japanese kinsoku rules.

use crate::config::{AsciiBreak, Config, JoinSpace};
use regex_lite::Regex;
use unicode_linebreak::{BreakClass, break_property, linebreaks};
use unicode_width::UnicodeWidthChar;

/// An ideographic character used as a stand-in for the ID line break class.
const ID_CHAR: char = '\u{4E00}';
/// A character of the NS (non-starter) line break class.
const NS_CHAR: char = '\u{3005}';
const IDEOGRAPHIC_SPACE: char = '\u{3000}';

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn trim_end_ws(s: &str) -> &str {
    s.trim_end_matches(is_ws)
}

pub struct Formatter<'a> {
    cfg: &'a Config,
    /// Width of East Asian ambiguous characters is 2.
    cjk: bool,
    comments: Vec<CommentDef>,
    list_re: Option<Regex>,
}

struct CommentDef {
    flags: String,
    text: String,
}

/// A line split into leader and text: `indent` + `com` + `mindent` + `text`.
struct Line<'l> {
    indent: &'l str,
    com: &'l str,
    mindent: &'l str,
    text: &'l str,
    flags: &'l str,
}

impl Line<'_> {
    fn is_blank(&self) -> bool {
        self.text.chars().all(is_ws)
    }

    fn leader_len(&self) -> usize {
        self.indent.len() + self.com.len() + self.mindent.len()
    }
}

impl<'a> Formatter<'a> {
    pub fn new(cfg: &'a Config, cjk: bool) -> Result<Formatter<'a>, String> {
        let comments = cfg
            .comments
            .iter()
            .filter_map(|c| {
                let (flags, text) = c.split_once(':')?;
                (!text.is_empty()).then(|| CommentDef {
                    flags: flags.to_string(),
                    text: text.to_string(),
                })
            })
            .collect();
        let list_re = if cfg.list_pattern.is_empty() {
            None
        } else {
            Some(Regex::new(&cfg.list_pattern).map_err(|e| format!("invalid list_pattern: {e}"))?)
        };
        Ok(Formatter {
            cfg,
            cjk,
            comments,
            list_re,
        })
    }

    fn char_width(&self, c: char, col: usize) -> usize {
        if c == '\t' {
            let ts = self.cfg.tabstop;
            return ts - col % ts;
        }
        let w = if self.cjk { c.width_cjk() } else { c.width() };
        w.unwrap_or(0)
    }

    fn str_width(&self, s: &str, start_col: usize) -> usize {
        s.chars()
            .fold(start_col, |col, c| col + self.char_width(c, col))
            - start_col
    }

    fn is_wide(&self, c: char) -> bool {
        self.char_width(c, 0) >= 2
    }

    /// Formats `lines` (without line terminators) and returns the output lines.
    pub fn format(&self, lines: &[&str]) -> Vec<String> {
        let parsed: Vec<Line> = lines.iter().map(|l| self.parse_leader(l)).collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < parsed.len() {
            if parsed[i].is_blank() {
                out.push(self.finish_line(lines[i].to_string()));
                i += 1;
                continue;
            }
            let start = i;
            i += 1;
            while i < parsed.len() && self.continues_paragraph(&parsed, start, i) {
                i += 1;
            }
            self.format_paragraph(&parsed[start..i], &mut out);
        }
        out
    }

    fn finish_line(&self, line: String) -> String {
        if self.cfg.trim_trailing {
            trim_end_ws(&line).to_string()
        } else {
            line
        }
    }

    fn parse_leader<'l>(&'l self, line: &'l str) -> Line<'l> {
        let indent_end = line.len() - line.trim_start_matches(is_ws).len();
        let mut com_end = indent_end;
        let mut flags = "";
        for def in &self.comments {
            if let Some(end) = self.match_comment(line, indent_end, def) {
                com_end = end;
                flags = &def.flags;
                if def.flags.contains('n') {
                    // Nested leaders such as "> > ".
                    'nested: loop {
                        let pos = com_end
                            + (line[com_end..].len()
                                - line[com_end..].trim_start_matches(is_ws).len());
                        for d in self.comments.iter().filter(|d| d.flags.contains('n')) {
                            if let Some(end) = self.match_comment(line, pos, d) {
                                com_end = end;
                                continue 'nested;
                            }
                        }
                        break;
                    }
                }
                break;
            }
        }
        let text_start =
            com_end + (line[com_end..].len() - line[com_end..].trim_start_matches(is_ws).len());
        Line {
            indent: &line[..indent_end],
            com: &line[indent_end..com_end],
            mindent: &line[com_end..text_start],
            text: &line[text_start..],
            flags,
        }
    }

    /// Returns the end of the comment leader `def` if `line` has it at `pos`.
    fn match_comment(&self, line: &str, pos: usize, def: &CommentDef) -> Option<usize> {
        let end = pos + def.text.len();
        if !line[pos..].starts_with(&def.text) {
            return None;
        }
        if def.flags.contains('b') && !line[end..].is_empty() && !line[end..].starts_with(is_ws) {
            return None;
        }
        Some(end)
    }

    fn list_marker<'l>(&self, text: &'l str) -> Option<&'l str> {
        let m = self.list_re.as_ref()?.find(text)?;
        (m.start() == 0 && !m.as_str().is_empty()).then(|| m.as_str())
    }

    /// Display width of the indent of a line, including a list marker.
    fn effective_indent(&self, line: &Line, raw: &str) -> usize {
        let leader = &raw[..line.leader_len()];
        let w = self.str_width(leader, 0);
        match self.list_marker(line.text) {
            Some(m) => w + self.str_width(m, w),
            None => w,
        }
    }

    fn continues_paragraph(&self, p: &[Line], start: usize, i: usize) -> bool {
        let (first, prev, cur) = (&p[start], &p[i - 1], &p[i]);
        if cur.is_blank() {
            return false;
        }
        if first.flags.contains('f') {
            if !cur.com.is_empty() {
                return false;
            }
        } else {
            let norm = |s: &str| s.chars().filter(|&c| !is_ws(c)).collect::<String>();
            if norm(prev.com) != norm(cur.com) {
                return false;
            }
        }
        if self.cfg.ideographic_space_paragraph && cur.text.starts_with(IDEOGRAPHIC_SPACE) {
            return false;
        }
        if self.list_marker(cur.text).is_some() {
            return false;
        }
        if self.cfg.second_line_indent {
            let prev_raw = line_raw(prev);
            let cur_raw = line_raw(cur);
            if self.effective_indent(cur, &cur_raw) > self.effective_indent(prev, &prev_raw) {
                return false;
            }
        }
        true
    }

    fn format_paragraph(&self, p: &[Line], out: &mut Vec<String>) {
        let first = &p[0];
        let lead1 = format!("{}{}{}", first.indent, first.com, first.mindent);
        let mut lead2 = if self.cfg.second_line_indent && p.len() >= 2 {
            format!("{}{}{}", p[1].indent, p[1].com, p[1].mindent)
        } else {
            self.continuation_leader(first, &lead1)
        };
        if self.cfg.expandtab {
            lead2 = self.expand_tabs(&lead2);
        }

        let mut text = first.text.to_string();
        for line in &p[1..] {
            text = self.join_line(trim_end_ws(&text), line.text);
        }

        let col1 = self.str_width(&lead1, 0);
        let col2 = self.str_width(&lead2, 0);
        for (k, seg) in self.wrap(&text, col1, col2).into_iter().enumerate() {
            let lead = if k == 0 { &lead1 } else { &lead2 };
            out.push(self.finish_line(format!("{lead}{seg}")));
        }
    }

    /// Builds the leader of continuation lines from the first line.
    fn continuation_leader(&self, first: &Line, lead1: &str) -> String {
        let list_w = self
            .list_marker(first.text)
            .map_or(0, |m| self.str_width(m, self.str_width(lead1, 0)));
        if first.flags.contains('f') {
            let w = self.str_width(lead1, 0) - self.str_width(first.indent, 0);
            format!("{}{}", first.indent, " ".repeat(w + list_w))
        } else {
            format!("{lead1}{}", " ".repeat(list_w))
        }
    }

    fn expand_tabs(&self, s: &str) -> String {
        let mut out = String::new();
        let mut col = 0;
        for c in s.chars() {
            let w = self.char_width(c, col);
            if c == '\t' {
                out.extend(std::iter::repeat_n(' ', w));
            } else {
                out.push(c);
            }
            col += w;
        }
        out
    }

    fn join_line(&self, a: &str, b: &str) -> String {
        let b = b.trim_start_matches(is_ws);
        let (Some(bc), Some(ac)) = (a.chars().next_back(), b.chars().next()) else {
            return format!("{a}{b}");
        };
        let space = if self.cfg.sentence_space && matches!(bc, '.' | '?' | '!') {
            "  "
        } else if matches!(bc, '、' | '。') {
            ""
        } else {
            let no_space = match self.cfg.join_space {
                JoinSpace::M => self.is_wide(bc) || self.is_wide(ac),
                JoinSpace::B => self.is_wide(bc) && self.is_wide(ac),
                JoinSpace::Always => false,
            };
            if no_space { "" } else { " " }
        };
        format!("{a}{space}{b}")
    }

    /// Returns `allowed` where `allowed[i]` tells whether a line may be broken
    /// before `chars[i]`.
    fn break_opportunities(&self, chars: &[char]) -> Vec<bool> {
        let n = chars.len();
        // Resolve the AI and CJ classes by substituting representative chars.
        let resolved: String = chars
            .iter()
            .map(|&c| match break_property(c as u32) {
                BreakClass::Ambiguous if self.cjk => ID_CHAR,
                BreakClass::Ambiguous => 'a',
                BreakClass::ConditionalJapaneseStarter if self.cfg.strict => NS_CHAR,
                BreakClass::ConditionalJapaneseStarter => ID_CHAR,
                _ => c,
            })
            .collect();
        let mut char_index = vec![usize::MAX; resolved.len() + 1];
        for (ci, (bi, _)) in resolved.char_indices().enumerate() {
            char_index[bi] = ci;
        }
        char_index[resolved.len()] = n;

        let mut allowed = vec![false; n + 1];
        for (bi, _) in linebreaks(&resolved) {
            allowed[char_index[bi]] = true;
        }
        allowed[0] = false;
        allowed[n] = true;

        for i in 1..n {
            let (a, b) = (chars[i - 1], chars[i]);
            if self.cfg.ascii_break == AsciiBreak::Space
                && a.is_ascii()
                && b.is_ascii()
                && !is_ws(a)
            {
                allowed[i] = false;
            }
            if self.cfg.no_line_start.contains(b) || self.cfg.no_line_end.contains(a) {
                allowed[i] = false;
            }
        }
        allowed
    }

    /// True if breaking before `chars[p]` would leave a one-letter word at the
    /// end of the line.
    fn after_single_letter(&self, chars: &[char], start: usize, p: usize) -> bool {
        if !self.cfg.no_single_letter_end {
            return false;
        }
        let mut k = p;
        while k > start && is_ws(chars[k - 1]) {
            k -= 1;
        }
        k > start && chars[k - 1].is_alphanumeric() && (k - 1 == start || is_ws(chars[k - 2]))
    }

    /// Wraps `text`; the first line starts at column `col1`, the others at `col2`.
    fn wrap(&self, text: &str, col1: usize, col2: usize) -> Vec<String> {
        let width = self.cfg.width;
        if width == 0 {
            return vec![text.to_string()];
        }
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let allowed = self.break_opportunities(&chars);
        let mut segs = Vec::new();
        let mut start = 0;
        let mut col0 = col1;
        loop {
            while start < n && is_ws(chars[start]) {
                start += 1;
            }
            if start >= n {
                break;
            }
            let end = self.find_break(&chars, &allowed, start, col0);
            let seg: String = chars[start..end].iter().collect();
            if end < n {
                segs.push(trim_end_ws(&seg).to_string());
            } else {
                segs.push(seg);
            }
            start = end;
            col0 = col2;
        }
        if segs.is_empty() {
            segs.push(String::new());
        }
        segs
    }

    /// Returns the end of the line starting at `chars[start]` (at column `col`).
    fn find_break(&self, chars: &[char], allowed: &[bool], start: usize, mut col: usize) -> usize {
        let n = chars.len();
        let (mut last_strong, mut last_any) = (None, None);
        for i in start..n {
            let c = chars[i];
            let w = self.char_width(c, col);
            if i > start && !is_ws(c) && col + w > self.cfg.width {
                return self.choose_break(chars, allowed, start, i, last_strong.or(last_any));
            }
            if i > start && allowed[i] {
                last_any = Some(i);
                if !self.after_single_letter(chars, start, i) {
                    last_strong = Some(i);
                }
            }
            col += w;
        }
        n
    }

    /// Decides where to break when `chars[i]` overflows the width.
    fn choose_break(
        &self,
        chars: &[char],
        allowed: &[bool],
        start: usize,
        i: usize,
        last: Option<usize>,
    ) -> usize {
        let n = chars.len();
        if allowed[i] && !(self.after_single_letter(chars, start, i) && last.is_some()) {
            return i;
        }
        // Hanging: keep up to `hang` chars prohibited at line start past the width.
        let hang_chars = &self.cfg.no_line_start;
        let mut j = i;
        while j < n && j - i < self.cfg.hang && hang_chars.contains(chars[j]) {
            j += 1;
        }
        if j > i {
            let mut k = j;
            while k < n && is_ws(chars[k]) {
                k += 1;
            }
            if k == n || allowed[k] {
                return k;
            }
        }
        // Push the preceding chars to the next line.
        if let Some(p) = last {
            return p;
        }
        // A word longer than the width: keep it whole.
        (i + 1..=n).find(|&q| allowed[q]).unwrap_or(n)
    }
}

/// Reassembles the leader and text of a parsed line.
fn line_raw(line: &Line) -> String {
    format!("{}{}{}{}", line.indent, line.com, line.mindent, line.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(cfg: &Config, input: &str) -> String {
        let f = Formatter::new(cfg, false).unwrap();
        let lines: Vec<&str> = input.lines().collect();
        f.format(&lines).join("\n")
    }

    fn cfg(width: usize) -> Config {
        Config {
            width,
            ..Config::default()
        }
    }

    #[test]
    fn wraps_english_at_spaces() {
        let c = cfg(30);
        assert_eq!(
            fmt(
                &c,
                "The quick brown fox jumps over the lazy dog. Supercalifragilisticexpialidocious words (like this) are long."
            ),
            "The quick brown fox jumps over\nthe lazy dog.\nSupercalifragilisticexpialidocious\nwords (like this) are long."
        );
    }

    #[test]
    fn matches_autofmt_japanese_without_hanging() {
        // Expected output from autofmt#japanese#formatexpr() with tw=30.
        let c = Config { hang: 0, ..cfg(30) };
        assert_eq!(
            fmt(
                &c,
                "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。「何でも薄暗いじめじめした所でニャーニャー泣いていた」事だけは記憶している。"
            ),
            "吾輩は猫である。名前はまだ無\nい。どこで生れたかとんと見当が\nつかぬ。「何でも薄暗いじめじめ\nした所でニャーニャー泣いてい\nた」事だけは記憶している。"
        );
        assert_eq!(
            fmt(
                &c,
                "VimはBram Moolenaar氏が開発したテキストエディタです。日本語と English が混在した文章（括弧付き）も、きちんと折り返せるでしょうか？ー長音ーや、っゃゅょの小書き文字。"
            ),
            "VimはBram Moolenaar氏が開発し\nたテキストエディタです。日本語\nと English が混在した文章（括\n弧付き）も、きちんと折り返せる\nでしょうか？ー長音ー\nや、っゃゅょの小書き文字。"
        );
    }

    #[test]
    fn hangs_punctuation() {
        let c = cfg(30);
        assert_eq!(
            fmt(
                &c,
                "吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。"
            ),
            "吾輩は猫である。名前はまだ無い。\nどこで生れたかとんと見当がつか\nぬ。"
        );
        // Two chars cannot hang with hang = 1: the preceding char goes along.
        assert_eq!(
            fmt(&cfg(10), "あいうえお。」かきく"),
            "あいうえ\nお。」かき\nく"
        );
        assert_eq!(
            fmt(&Config { hang: 2, ..cfg(10) }, "あいうえお。」かきく"),
            "あいうえお。」\nかきく"
        );
    }

    #[test]
    fn pushes_out_opening_brackets() {
        assert_eq!(fmt(&cfg(10), "あいうえ「おか」"), "あいうえ\n「おか」");
    }

    #[test]
    fn keeps_long_words_whole() {
        assert_eq!(
            fmt(
                &cfg(20),
                "詳細は https://example.com/very/long/path を参照。"
            ),
            "詳細は\nhttps://example.com/very/long/path\nを参照。"
        );
        assert_eq!(
            fmt(&cfg(10), "https://example.com/x。」"),
            "https://example.com/x。」"
        );
    }

    #[test]
    fn strict_controls_small_kana() {
        let text = "あいうえおかきくけこさしすせそたちつてとなにぬねのはっょんスーパー";
        assert_eq!(
            fmt(&Config { hang: 0, ..cfg(30) }, text),
            "あいうえおかきくけこさしすせそ\nたちつてとなにぬねのはっょん\nスーパー"
        );
        let mut c = Config {
            hang: 0,
            strict: false,
            ..cfg(30)
        };
        c.no_line_start.retain(|ch| ch != 'ー');
        assert_eq!(
            fmt(&c, text),
            "あいうえおかきくけこさしすせそ\nたちつてとなにぬねのはっょんス\nーパー"
        );
    }

    #[test]
    fn joins_lines() {
        let c = cfg(80);
        assert_eq!(
            fmt(&c, "これは\n文章です。\n続き\nand\nmore\n日本語"),
            "これは文章です。続きand more日本語"
        );
        let c = Config {
            join_space: JoinSpace::Always,
            ..cfg(80)
        };
        assert_eq!(fmt(&c, "これは\n文章です。\n続き"), "これは 文章です。続き");
        let c = Config {
            sentence_space: true,
            ..cfg(80)
        };
        assert_eq!(fmt(&c, "End.\nNext"), "End.  Next");
    }

    #[test]
    fn splits_paragraphs() {
        let c = cfg(30);
        assert_eq!(
            fmt(
                &c,
                "これは一段落目の文章です。\n改行で続いています。\n　全角スペースで始まる行は新段落。\nここは続き。\n\nnext"
            ),
            "これは一段落目の文章です。改行\nで続いています。\n　全角スペースで始まる行は新段\n落。ここは続き。\n\nnext"
        );
    }

    #[test]
    fn handles_lists_and_indent() {
        let c = cfg(20);
        assert_eq!(
            fmt(
                &c,
                "1. first item is long enough to wrap\n2. second\n- bullet item that wraps too"
            ),
            "1. first item is\n   long enough to\n   wrap\n2. second\n- bullet item that\n  wraps too"
        );
        assert_eq!(
            fmt(&c, "    aa bb cc dd ee ff\n  gg hh ii jj kk ll mm"),
            "    aa bb cc dd ee\n  ff gg hh ii jj kk\n  ll mm"
        );
    }

    #[test]
    fn handles_comment_leaders() {
        let c = cfg(20);
        assert_eq!(
            fmt(
                &c,
                "> > quoted text that is long\n> > more\n# comment that should wrap here"
            ),
            "> > quoted text that\n> > is long more\n# comment that\n# should wrap here"
        );
        assert_eq!(
            fmt(&c, "#include <stdio.h> and a lot more"),
            "#include <stdio.h>\nand a lot more"
        );
    }

    #[test]
    fn single_letter_words() {
        let c = Config {
            no_single_letter_end: true,
            ..cfg(12)
        };
        assert_eq!(fmt(&c, "This is a pen of mine"), "This is\na pen of\nmine");
        assert_eq!(
            fmt(&cfg(12), "This is a pen of mine"),
            "This is a\npen of mine"
        );
    }

    #[test]
    fn width_zero_only_joins() {
        assert_eq!(fmt(&cfg(0), "aaa\nbbb\n\nccc"), "aaa bbb\n\nccc");
    }

    #[test]
    fn trailing_whitespace() {
        assert_eq!(fmt(&cfg(80), "abc   \n   \n"), "abc\n");
        let c = Config {
            trim_trailing: false,
            ..cfg(80)
        };
        assert_eq!(fmt(&c, "abc   \n   "), "abc   \n   ");
    }

    #[test]
    fn ambiguous_width() {
        let c = Config { hang: 0, ..cfg(10) };
        let f = Formatter::new(&c, true).unwrap();
        assert_eq!(f.format(&["○○○○○○"]), vec!["○○○○○", "○"]);
        let f = Formatter::new(&c, false).unwrap();
        assert_eq!(f.format(&["○○○○○○"]), vec!["○○○○○○"]);
    }
}
