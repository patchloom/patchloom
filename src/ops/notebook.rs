//! Replace one Jupyter cell's source by its `id`.
//!
//! The new body is a normal string. It is stored as an nbformat `source`
//! array of lines. The rest of the file, including outputs, is left in place.

use crate::exit::{AmbiguousError, InvalidInputError, NoMatchError, ParseErrorError};
use serde_json::Value;

/// Split a cell body into nbformat source lines.
///
/// A trailing newline stays on the last line. An empty body is an empty array.
pub(crate) fn source_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('\n') {
        let (line, after) = rest.split_at(i + 1);
        lines.push(line.to_string());
        rest = after;
    }
    if !rest.is_empty() {
        lines.push(rest.to_string());
    }
    lines
}

fn source_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect::<String>(),
        _ => String::new(),
    }
}

/// Replace the source of the cell whose `id` is `cell_id`.
///
/// Returns the original text when the cell body is already `new_source`.
pub(crate) fn replace_cell_source(
    raw: &str,
    cell_id: &str,
    new_source: &str,
) -> anyhow::Result<String> {
    if cell_id.is_empty() {
        return Err(InvalidInputError {
            msg: "notebook.edit cell_id is empty".to_string(),
        }
        .into());
    }
    let value: Value = serde_json::from_str(raw).map_err(|e| ParseErrorError {
        msg: format!("notebook is not valid JSON: {e}"),
    })?;
    let cells = value
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| InvalidInputError {
            msg: "notebook has no cells array".to_string(),
        })?;
    let hits: Vec<usize> = cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.get("id").and_then(Value::as_str) == Some(cell_id))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [] => Err(NoMatchError {
            msg: format!("notebook cell id not found: {cell_id}"),
        }
        .into()),
        [_, _, ..] => Err(AmbiguousError {
            msg: format!("notebook cell id is not unique: {cell_id}"),
        }
        .into()),
        &[index] => {
            let current = cells[index]
                .get("source")
                .map(source_text)
                .unwrap_or_default();
            if current == new_source {
                return Ok(raw.to_string());
            }
            let span = source_value_span(raw, index)?;
            let encoded =
                serde_json::to_string(&source_lines(new_source)).map_err(|e| ParseErrorError {
                    msg: format!("could not encode notebook cell source: {e}"),
                })?;
            let mut out = String::with_capacity(raw.len() + encoded.len());
            out.push_str(&raw[..span.0]);
            out.push_str(&encoded);
            out.push_str(&raw[span.1..]);
            Ok(out)
        }
    }
}

/// Byte range of the `source` value inside the `index`th cell object.
fn source_value_span(raw: &str, index: usize) -> anyhow::Result<(usize, usize)> {
    let mut p = Parser { s: raw, i: 0 };
    p.skip_ws();
    let spans = p.root_cell_spans().map_err(|msg| ParseErrorError { msg })?;
    let cell = spans.get(index).ok_or_else(|| ParseErrorError {
        msg: "notebook cells array does not match the parsed cell count".to_string(),
    })?;
    let mut inner = Parser {
        s: &raw[cell.0..cell.1],
        i: 0,
    };
    let rel = inner
        .object_key_value_span("source")
        .map_err(|msg| InvalidInputError {
            msg: format!("notebook cell has no source field ({msg})"),
        })?;
    Ok((cell.0 + rel.0, cell.0 + rel.1))
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while let Some(c) = self.s[self.i..].chars().next() {
            if c.is_whitespace() {
                self.i += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }

    fn err(&self, msg: &str) -> Result<(), String> {
        Err(format!("{msg} at byte {}", self.i))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.bump() != Some('"') {
            self.err("expected string")?;
        }
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(out),
                Some('\\') => {
                    let esc = self
                        .bump()
                        .ok_or_else(|| "truncated string escape".to_string())?;
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hex = self
                                .s
                                .get(self.i..self.i + 4)
                                .ok_or_else(|| "truncated unicode escape".to_string())?;
                            let code = u32::from_str_radix(hex, 16)
                                .map_err(|_| format!("bad unicode escape {hex}"))?;
                            self.i += 4;
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        other => return Err(format!("bad string escape {other}")),
                    }
                }
                Some(c) => out.push(c),
                None => return Err("unterminated string".to_string()),
            }
        }
    }

    fn skip_string(&mut self) -> Result<(), String> {
        self.parse_string().map(|_| ())
    }

    fn skip_literal(&mut self, lit: &str) -> Result<(), String> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(format!("expected {lit}"))
        }
    }

    fn skip_number(&mut self) -> Result<(), String> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.bump();
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek() == Some('.') {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        if self.i == start {
            self.err("expected number")
        } else {
            Ok(())
        }
    }

    fn skip_value(&mut self) -> Result<(), String> {
        self.skip_ws();
        match self.peek() {
            Some('"') => self.skip_string(),
            Some('{') => self.skip_object(),
            Some('[') => self.skip_array(),
            Some('t') => self.skip_literal("true"),
            Some('f') => self.skip_literal("false"),
            Some('n') => self.skip_literal("null"),
            Some('-') | Some('0'..='9') => self.skip_number(),
            _ => self.err("expected value"),
        }
    }

    fn skip_object(&mut self) -> Result<(), String> {
        if self.bump() != Some('{') {
            self.err("expected object")?;
        }
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(());
        }
        loop {
            self.skip_ws();
            self.skip_string()?;
            self.skip_ws();
            if self.bump() != Some(':') {
                self.err("expected colon")?;
            }
            self.skip_value()?;
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => return Ok(()),
                _ => return Err("expected comma or end of object".to_string()),
            }
        }
    }

    fn skip_array(&mut self) -> Result<(), String> {
        if self.bump() != Some('[') {
            self.err("expected array")?;
        }
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(());
        }
        loop {
            self.skip_value()?;
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some(']') => return Ok(()),
                _ => return self.err("expected comma or end of array"),
            }
        }
    }

    /// Spans of each value in the root object's `cells` array.
    fn root_cell_spans(&mut self) -> Result<Vec<(usize, usize)>, String> {
        self.skip_ws();
        if self.bump() != Some('{') {
            self.err("notebook root is not an object")?;
        }
        self.skip_ws();
        if self.peek() == Some('}') {
            return Err("notebook has no cells array".to_string());
        }
        // serde_json and Jupyter keep the last duplicate key.
        let mut found: Option<Vec<(usize, usize)>> = None;
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.bump() != Some(':') {
                self.err("expected colon")?;
            }
            self.skip_ws();
            if key == "cells" {
                found = Some(self.array_element_spans()?);
            } else {
                self.skip_value()?;
            }
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => {
                    return found.ok_or_else(|| "notebook has no cells array".to_string());
                }
                _ => return Err("expected comma or end of object".to_string()),
            }
        }
    }

    fn array_element_spans(&mut self) -> Result<Vec<(usize, usize)>, String> {
        if self.bump() != Some('[') {
            return Err("notebook cells is not an array".to_string());
        }
        let mut spans = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(spans);
        }
        loop {
            self.skip_ws();
            let start = self.i;
            self.skip_value()?;
            spans.push((start, self.i));
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some(']') => return Ok(spans),
                _ => return Err("expected comma or end of array".to_string()),
            }
        }
    }

    /// Span of the value for `want` inside the object at the current position.
    fn object_key_value_span(&mut self, want: &str) -> Result<(usize, usize), String> {
        self.skip_ws();
        if self.bump() != Some('{') {
            return Err("cell is not an object".to_string());
        }
        self.skip_ws();
        if self.peek() == Some('}') {
            return Err(format!("missing {want}"));
        }
        // Last duplicate key wins, matching serde_json and Jupyter.
        let mut found: Option<(usize, usize)> = None;
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.bump() != Some(':') {
                self.err("expected colon")?;
            }
            self.skip_ws();
            if key == want {
                let start = self.i;
                self.skip_value()?;
                found = Some((start, self.i));
            } else {
                self.skip_value()?;
            }
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => {
                    return found.ok_or_else(|| format!("missing {want}"));
                }
                _ => return Err("expected comma or end of object".to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTEBOOK: &str = r#"{
 "cells": [
  {
   "cell_type": "markdown",
   "id": "intro",
   "metadata": {},
   "source": ["Title\n"]
  },
  {
   "cell_type": "code",
   "execution_count": 1,
   "id": "load",
   "metadata": {},
   "outputs": [{"output_type": "stream", "name": "stdout", "text": ["ok\n"]}],
   "source": ["import pandas\n"]
  }
 ],
 "nbformat": 4,
 "nbformat_minor": 5
}
"#;

    #[test]
    fn replace_writes_source_as_line_array_and_keeps_outputs() {
        let out =
            replace_cell_source(NOTEBOOK, "load", "import pandas as pd\ndf.head()\n").unwrap();
        assert!(out.contains("import pandas as pd\\n"));
        assert!(out.contains("df.head()\\n"));
        assert!(out.contains("\"text\": [\"ok\\n\"]"));
        assert!(out.contains("\"id\": \"intro\""));
        assert!(out.contains("Title\\n"));
        let v: Value = serde_json::from_str(&out).unwrap();
        let source = &v["cells"][1]["source"];
        assert_eq!(source[0], "import pandas as pd\n");
        assert_eq!(source[1], "df.head()\n");
        assert_eq!(v["cells"][1]["cell_type"], "code");
    }

    #[test]
    fn same_body_does_not_rewrite() {
        let out = replace_cell_source(NOTEBOOK, "load", "import pandas\n").unwrap();
        assert_eq!(out, NOTEBOOK);
    }

    #[test]
    fn missing_id_is_no_match() {
        let err = replace_cell_source(NOTEBOOK, "missing", "x\n").unwrap_err();
        assert!(err.downcast_ref::<NoMatchError>().is_some());
    }

    #[test]
    fn duplicate_id_is_ambiguous() {
        let raw = NOTEBOOK.replace("\"id\": \"intro\"", "\"id\": \"load\"");
        let err = replace_cell_source(&raw, "load", "x\n").unwrap_err();
        assert!(err.downcast_ref::<AmbiguousError>().is_some());
    }

    #[test]
    fn invalid_json_is_parse_error() {
        let err = replace_cell_source("{", "load", "x").unwrap_err();
        assert!(err.downcast_ref::<ParseErrorError>().is_some());
    }

    #[test]
    fn empty_cell_id_is_invalid_input() {
        let err = replace_cell_source(NOTEBOOK, "", "x").unwrap_err();
        assert!(err.downcast_ref::<InvalidInputError>().is_some());
    }

    #[test]
    fn source_stored_as_one_string_is_replaced() {
        let raw = r#"{"cells":[{"cell_type":"code","id":"a","source":"old\n"}],"nbformat":4}"#;
        let out = replace_cell_source(raw, "a", "new\n").unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cells"][0]["source"][0], "new\n");
        assert!(out.contains("\"cell_type\":\"code\"") || out.contains("\"cell_type\": \"code\""));
    }

    #[test]
    fn last_cells_array_is_the_one_edited() {
        let raw = r#"{"cells":[{"id":"old","source":"nope\n"}],"cells":[{"id":"live","cell_type":"code","source":"keep\n"}]}"#;
        let out = replace_cell_source(raw, "live", "next\n").unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cells"][0]["id"], "live");
        assert_eq!(v["cells"][0]["source"][0], "next\n");
        assert!(
            out.contains("nope"),
            "discarded cells array should stay: {out}"
        );
    }

    #[test]
    fn last_source_key_is_the_one_edited() {
        let raw = r#"{"cells":[{"id":"a","source":"shadow\n","source":"live\n"}]}"#;
        let out = replace_cell_source(raw, "a", "next\n").unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cells"][0]["source"][0], "next\n");
        assert!(out.contains("shadow"), "first source should stay: {out}");
    }
}
