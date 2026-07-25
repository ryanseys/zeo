//! Numeric/symbol/string/xstring/regexp literal lowering: the magic-comment
//! encoding-const mapping, source-offset line lookup for `__LINE__`, and the
//! interpolated-literal (string/symbol/xstring/regexp) part family shared by
//! all four. Split out of `parse/mod.rs`.

use super::{PResult, lower_node};
use crate::hir::{Hir, HirNode, StrPart};
use ruby_prism::{Node, ParseResult};

/// Maps a magic-comment encoding name to its `Encoding::` constant spelling
/// (`None` = the UTF-8 default, needing no override), rejecting an
/// unsupported encoding with a clean compile error.
pub fn encoding_const_name(name: &str) -> PResult<Option<&'static str>> {
    let norm: String = name
        .chars()
        .filter(|c| *c != '-' && *c != '_' && *c != '.')
        .flat_map(char::to_lowercase)
        .collect();
    Ok(match norm.as_str() {
        "utf8" | "cp65001" => None,
        "usascii" | "ascii" | "ansix341968" | "646" => Some("US_ASCII"),
        "ascii8bit" | "binary" => Some("ASCII_8BIT"),
        "iso88591" | "latin1" => Some("ISO_8859_1"),
        _ => {
            return Err(format!(
                "unsupported source encoding in magic comment: '{name}' \
                 (supported: UTF-8, US-ASCII, ASCII-8BIT/BINARY, ISO-8859-1)"
            )
            .into());
        }
    })
}

/// The 1-based line a byte offset falls on. `Location` only carries
/// offsets, so this counts the newlines before it -- fine for the handful
/// of `__LINE__`/`__dir__` sites a program has (this is not on any hot
/// path; it runs once per occurrence, at compile time).
pub(crate) fn line_of(result: &ParseResult, offset: usize) -> i64 {
    let src = result.source();
    1 + src[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as i64
}

/// A literal string segment's bytes as a `StrPart`: readable UTF-8 text when
/// the bytes form valid UTF-8 (the overwhelmingly common case), else the raw
/// bytes preserved for the encoding engine (a `"\xNN"` escape that isn't a
/// character -- Ruby tags such a literal ASCII-8BIT).
pub(crate) fn string_literal_part(bytes: &[u8]) -> StrPart {
    match std::str::from_utf8(bytes) {
        Ok(s) => StrPart::Lit(s.to_string()),
        Err(_) => StrPart::Bytes(bytes.to_vec()),
    }
}

/// Lower an interpolated literal's parts, FLATTENING any nested interpolated
/// string into the outer list.
///
/// A part is not always a leaf: backslash-continued adjacent literals
/// (`"<a w='#{px}' " \ "h='#{px}'>"`) parse as an `InterpolatedStringNode`
/// whose own parts are themselves `InterpolatedStringNode`s. Splicing the
/// inner parts in is exactly the concatenation the source spells, and it
/// composes to any nesting depth. Shared by the string, symbol, and regexp
/// literal paths, all of which can carry the same adjacency.
pub(crate) fn lower_string_parts<'a>(
    result: &ParseResult,
    hir: &mut Hir,
    parts: impl Iterator<Item = Node<'a>>,
) -> PResult<Vec<StrPart>> {
    let mut out = Vec::new();
    for part in parts {
        match part.as_interpolated_string_node() {
            Some(inner) => out.extend(lower_string_parts(result, hir, inner.parts().iter())?),
            None => out.push(lower_string_part(result, hir, &part)?),
        }
    }
    Ok(out)
}

/// One `parts()` entry of an `InterpolatedStringNode`:
///
/// - a literal chunk (`StringNode`);
/// - an `#{ }` (`EmbeddedStatementsNode`) -- several statements answer the
///   LAST, via the same `Seq` a parenthesized `(a; b)` lowers to, and an
///   EMPTY `#{}` interpolates the empty string (real Ruby: `"x#{}y"` is
///   `"xy"`);
/// - a brace-less `#@ivar`/`#@@cvar`/`#$global` (`EmbeddedVariableNode`),
///   whose `variable()` is an ordinary read node and so needs no special
///   handling beyond unwrapping it.
fn lower_string_part(result: &ParseResult, hir: &mut Hir, node: &Node<'_>) -> PResult<StrPart> {
    if let Some(s) = node.as_string_node() {
        return Ok(string_literal_part(s.unescaped()));
    }
    if let Some(embedded) = node.as_embedded_statements_node() {
        let stmts: Vec<_> = embedded
            .statements()
            .map(|s| s.body().iter().collect())
            .unwrap_or_default();
        return match stmts.as_slice() {
            [] => Ok(StrPart::Lit(String::new())),
            [only] => Ok(StrPart::Interp(lower_node(result, hir, only)?)),
            _ => {
                let ids = stmts
                    .iter()
                    .map(|s| lower_node(result, hir, s))
                    .collect::<PResult<Vec<_>>>()?;
                Ok(StrPart::Interp(hir.push(HirNode::Seq(ids))))
            }
        };
    }
    if let Some(embedded) = node.as_embedded_variable_node() {
        return Ok(StrPart::Interp(lower_node(
            result,
            hir,
            &embedded.variable(),
        )?));
    }
    Err("unsupported string interpolation part (zeo limitation)"
        .to_string()
        .into())
}
