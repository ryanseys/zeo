//! Numeric/symbol/string/xstring/regexp literal lowering: the magic-comment
//! encoding-const mapping, source-offset line lookup for `__LINE__`, and the
//! interpolated-literal (string/symbol/xstring/regexp) part family shared by
//! all four. Split out of `parse/mod.rs`.

use super::{PResult, lower_array_elem, lower_kwargs, lower_node};
use crate::hir::{ArrayElem, Hir, HirNode, NodeId, RegexpFlags, StrPart};
use ruby_prism::{Node, ParseResult};

/// The literal/collection family of [`super::lower_node_inner`]'s recognizer
/// chain: numerics, symbols, the string-literal shapes (plain, interpolated,
/// xstring, regexp), `nil`/`true`/`false`/`self`, and the collection literals
/// (array, hash, range, flip-flop). `Ok(None)` = not this family's node; the
/// chain in `mod.rs` moves on.
pub(crate) fn try_lower(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
) -> PResult<Option<NodeId>> {
    if let Some(int) = node.as_integer_node() {
        // prism's own arbitrary-precision value (LSB-first u32 digits) --
        // which also handles `0xff`/`0b101`/`1_000` uniformly, unlike the
        // old source-text `parse::<i64>()`. Values that fit stay the
        // ordinary `IntegerLit(i64)`; anything bigger is a bignum literal.
        let value = int.value();
        let (negative, digits) = value.to_u32_digits();
        return Ok(Some(hir.push(match assemble_i64(negative, digits) {
            Some(v) => HirNode::IntegerLit(v),
            None => HirNode::BigIntegerLit {
                negative,
                digits: digits.to_vec(),
            },
        })));
    }

    if let Some(float) = node.as_float_node() {
        return Ok(Some(hir.push(HirNode::FloatLit(float.value()))));
    }

    if let Some(rat) = node.as_rational_node() {
        // prism pre-rationalizes: `1.5r` arrives numerator 3, denominator
        // 2. The numerator carries the sign; the denominator is positive
        // and non-zero by syntax.
        let numerator = rat.numerator();
        let denominator = rat.denominator();
        let (negative, num_digits) = numerator.to_u32_digits();
        let (_, den_digits) = denominator.to_u32_digits();
        return Ok(Some(hir.push(HirNode::RationalLit {
            negative,
            num_digits: num_digits.to_vec(),
            den_digits: den_digits.to_vec(),
        })));
    }

    if let Some(im) = node.as_imaginary_node() {
        let inner = lower_node(result, hir, &im.numeric())?;
        return Ok(Some(hir.push(HirNode::ImaginaryLit(inner))));
    }

    if let Some(sym) = node.as_symbol_node() {
        let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
        return Ok(Some(hir.push(HirNode::SymbolLit(name))));
    }

    if node.as_nil_node().is_some() {
        return Ok(Some(hir.push(HirNode::NilLit)));
    }
    if node.as_true_node().is_some() {
        return Ok(Some(hir.push(HirNode::BoolLit(true))));
    }
    if node.as_false_node().is_some() {
        return Ok(Some(hir.push(HirNode::BoolLit(false))));
    }
    if node.as_self_node().is_some() {
        return Ok(Some(hir.push(HirNode::SelfRef)));
    }

    if let Some(s) = node.as_string_node() {
        return Ok(Some(hir.push(HirNode::StringLit(vec![
            string_literal_part(s.unescaped()),
        ]))));
    }

    // `:"hello_#{x}"` -- an interpolated symbol is exactly its interpolated
    // STRING, interned. Lowered as that string plus a `to_sym` call rather
    // than given its own HIR node: the parts are the same shape, and the
    // name isn't known until runtime anyway, so there is nothing a
    // dedicated node could do that this doesn't.
    if let Some(isym) = node.as_interpolated_symbol_node() {
        let parts = lower_string_parts(result, hir, isym.parts().iter())?;
        let text = hir.push(HirNode::StringLit(parts));
        return Ok(Some(hir.push(HirNode::Call {
            receiver: Some(text),
            name: "to_sym".to_string(),
            args: Vec::new(),
            kwargs: Vec::new(),
            block: None,
            block_arg: None,
            safe: false,
        })));
    }

    if let Some(istr) = node.as_interpolated_string_node() {
        let parts = lower_string_parts(result, hir, istr.parts().iter())?;
        return Ok(Some(hir.push(HirNode::StringLit(parts))));
    }

    // `` `cmd` `` / `%x{cmd}` (and the interpolated form) -- CRuby compiles
    // both to `putself` + an ordinary send of `` :` `` with the command
    // String as its one argument (compile.c), so they are the exact
    // string-literal shapes above wrapped in an implicit-self fcall to the
    // overridable `Kernel#\``. Not a direct syscall: a user who reopens
    // `Kernel#\`` (or defines `` def `(cmd) ``) wins, real Ruby's rule.
    if let Some(xs) = node.as_x_string_node() {
        let cmd = hir.push(HirNode::StringLit(vec![string_literal_part(
            xs.unescaped(),
        )]));
        return Ok(Some(backtick_call(hir, cmd)));
    }

    if let Some(xs) = node.as_interpolated_x_string_node() {
        let parts = lower_string_parts(result, hir, xs.parts().iter())?;
        let cmd = hir.push(HirNode::StringLit(parts));
        return Ok(Some(backtick_call(hir, cmd)));
    }

    // `/pattern/flags` / `%r{pattern}flags` (`RegularExpressionNode` covers
    // BOTH delimiter spellings -- prism only distinguishes opening/closing
    // `Location`s, not a separate node kind). `o` is the one genuinely
    // ignorable flag: "only interpolate once" has no effect when every regexp
    // literal is freshly constructed anyway.
    if let Some(re) = node.as_regular_expression_node() {
        let content = String::from_utf8_lossy(re.unescaped()).into_owned();
        let encoding = forced_regexp_encoding(RegexpEncodingFlags {
            ascii_8bit: re.is_ascii_8bit(),
            euc_jp: re.is_euc_jp(),
            windows_31j: re.is_windows_31j(),
            utf_8: re.is_utf_8(),
        });
        return Ok(Some(hir.push(HirNode::RegexpLit(
            vec![StrPart::Lit(content)],
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
                encoding,
                once: false,
            },
        ))));
    }

    if let Some(re) = node.as_interpolated_regular_expression_node() {
        let parts = lower_string_parts(result, hir, re.parts().iter())?;
        let encoding = forced_regexp_encoding(RegexpEncodingFlags {
            ascii_8bit: re.is_ascii_8bit(),
            euc_jp: re.is_euc_jp(),
            windows_31j: re.is_windows_31j(),
            utf_8: re.is_utf_8(),
        });
        return Ok(Some(hir.push(HirNode::RegexpLit(
            parts,
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
                encoding,
                once: re.is_once(),
            },
        ))));
    }

    // A bare regexp written where a condition goes (`if /foo/`) matches
    // against `$_` and answers the position or nil. `Regexp#~` is that
    // operator and reads the same global, so the literal plus one send is
    // the whole form.
    if let Some(re) = node.as_match_last_line_node() {
        let content = String::from_utf8_lossy(re.unescaped()).into_owned();
        let encoding = forced_regexp_encoding(RegexpEncodingFlags {
            ascii_8bit: re.is_ascii_8bit(),
            euc_jp: re.is_euc_jp(),
            windows_31j: re.is_windows_31j(),
            utf_8: re.is_utf_8(),
        });
        let lit = hir.push(HirNode::RegexpLit(
            vec![StrPart::Lit(content)],
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
                encoding,
                once: false,
            },
        ));
        return Ok(Some(match_last_line(hir, lit)));
    }

    if let Some(re) = node.as_interpolated_match_last_line_node() {
        let parts = lower_string_parts(result, hir, re.parts().iter())?;
        let encoding = forced_regexp_encoding(RegexpEncodingFlags {
            ascii_8bit: re.is_ascii_8bit(),
            euc_jp: re.is_euc_jp(),
            windows_31j: re.is_windows_31j(),
            utf_8: re.is_utf_8(),
        });
        let lit = hir.push(HirNode::RegexpLit(
            parts,
            RegexpFlags {
                ignore_case: re.is_ignore_case(),
                extended: re.is_extended(),
                multiline: re.is_multi_line(),
                encoding,
                // Ruby builds a bare condition's pattern every time, `/o`
                // or not.
                once: false,
            },
        ));
        return Ok(Some(match_last_line(hir, lit)));
    }

    if let Some(arr) = node.as_array_node() {
        let elements = arr
            .elements()
            .iter()
            .map(|el| lower_array_elem(result, hir, &el))
            .collect::<PResult<Vec<_>>>()?;
        return Ok(Some(hir.push(HirNode::ArrayLit(elements))));
    }

    if let Some(h) = node.as_hash_node() {
        let elements: Vec<Node<'_>> = h.elements().iter().collect();
        let kwargs = lower_kwargs(result, hir, &elements)?;
        return Ok(Some(hir.push(HirNode::HashLit(kwargs))));
    }

    // The BRACE-LESS hash as the last element of an ARRAY literal
    // (`[a, b, from: x, to: y]`). Ruby collapses it into one ordinary Hash
    // there -- `[1, k: 2]` has two elements, not three -- so it lowers exactly
    // like the braced form above; prism just spells it differently.
    //
    // A CALL's trailing hash never reaches here: `lower_call_args` peels it
    // off first, because in that position it is keyword arguments rather than
    // a value, and the two bind differently.
    if let Some(kw) = node.as_keyword_hash_node() {
        let elements: Vec<Node<'_>> = kw.elements().iter().collect();
        let kwargs = lower_kwargs(result, hir, &elements)?;
        return Ok(Some(hir.push(HirNode::HashLit(kwargs))));
    }

    // A `..`/`...` prism decided is a CONDITION, not a Range -- see
    // `HirNode::FlipFlop`. An omitted side lowers to nil, which is falsy, and
    // that is exactly Ruby's behaviour for a one-sided flip-flop.
    if let Some(ff) = node.as_flip_flop_node() {
        let mut side = |n: Option<Node<'_>>| match n {
            None => Ok(hir.push(HirNode::NilLit)),
            Some(n) => lower_node(result, hir, &n),
        };
        let left = side(ff.left())?;
        let right = side(ff.right())?;
        let state = hir.flip_flops;
        hir.flip_flops += 1;
        return Ok(Some(hir.push(HirNode::FlipFlop {
            state,
            left,
            right,
            exclusive: ff.is_exclude_end(),
        })));
    }

    if let Some(range) = node.as_range_node() {
        let start = match range.left() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        let end = match range.right() {
            None => None,
            Some(n) => Some(lower_node(result, hir, &n)?),
        };
        return Ok(Some(hir.push(HirNode::RangeLit {
            start,
            end,
            exclusive: range.is_exclude_end(),
        })));
    }

    Ok(None)
}

/// The implicit-self fcall of `` Kernel#` `` both xstring shapes share.
fn backtick_call(hir: &mut Hir, cmd: NodeId) -> NodeId {
    hir.push(HirNode::Call {
        receiver: None,
        name: "`".to_string(),
        args: vec![ArrayElem::Single(cmd)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// `re =~ $_` -- what `if /foo/` means. NOT `Regexp#~`, which spells the same
/// idea but answers nil for a `$_` that is not a String where `=~` raises the
/// `TypeError` ruby raises here.
fn match_last_line(hir: &mut Hir, regexp: NodeId) -> NodeId {
    let line = hir.push(HirNode::GlobalRead("$_".to_string()));
    hir.push(HirNode::Call {
        receiver: Some(regexp),
        name: "=~".to_string(),
        args: vec![ArrayElem::Single(line)],
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// Assembles prism's `(negative, LSB-first u32 digits)` integer shape into
/// an `i64` when it fits (`None` = a bignum literal).
pub(crate) fn assemble_i64(negative: bool, digits: &[u32]) -> Option<i64> {
    let mut magnitude: u64 = 0;
    for (i, &d) in digits.iter().enumerate() {
        if i >= 2 {
            if d != 0 {
                return None;
            }
            continue;
        }
        magnitude |= u64::from(d) << (32 * i);
    }
    if negative {
        // i64::MIN's magnitude is representable; anything larger isn't.
        if magnitude > (i64::MAX as u64) + 1 {
            return None;
        }
        Some((magnitude as i128).wrapping_neg() as i64)
    } else {
        i64::try_from(magnitude).ok()
    }
}

/// The regexp-literal encoding FLAGS (`/x/n`, `/x/e`, `/x/s`, `/x/u`) as the
/// abi enum. A pattern with none of them takes the SOURCE encoding.
///
/// A flag that CONTRADICTS the pattern's own bytes is rejected when they
/// genuinely differ -- and ruby refuses to guess which was meant, in the
/// PARSER (`regexp encoding option 'e' differs from source encoding
/// 'UTF-8'`, a `SyntaxError`). prism is that parser, so
/// `parse_and_lower_into` has already turned those away by the time this
/// runs; there is nothing left here to reject, and re-deriving the rule
/// would only be a second, worse copy of it.
///
/// ruby2ruby and ruby_parser both open with `ENC_EUC = /x/e.options` -- a
/// throwaway ASCII pattern whose only purpose is the flag bits.
struct RegexpEncodingFlags {
    ascii_8bit: bool,
    euc_jp: bool,
    windows_31j: bool,
    utf_8: bool,
}

fn forced_regexp_encoding(flags: RegexpEncodingFlags) -> zeo_abi::RegexpEncoding {
    use zeo_abi::RegexpEncoding;
    if flags.ascii_8bit {
        RegexpEncoding::None
    } else if flags.euc_jp {
        RegexpEncoding::EucJp
    } else if flags.windows_31j {
        RegexpEncoding::Windows31j
    } else if flags.utf_8 {
        RegexpEncoding::Utf8
    } else {
        RegexpEncoding::Source
    }
}

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
    let counted = 1 + src[..offset.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as i64;
    counted + i64::from(super::context::current_line_offset())
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
