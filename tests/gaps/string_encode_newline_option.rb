# `String#encode`'s newline-rewrite options are ignored: zeo returns the
# input unchanged for `newline: :cr`/`:crlf` and the `cr_newline:`/
# `crlf_newline:` spellings, where ruby rewrites the newlines.
# `universal_newline: true` already agrees. Hypothesis: the encode option
# parser (`builtins/string.rs::parse_encode_opts`) reads the universal
# option only. (Found by the 2026-08-24 probe sweep.)
p "a\nb".encode("UTF-8", newline: :cr)
p "a\nb".encode("UTF-8", newline: :crlf)
p "a\nb".encode("UTF-8", crlf_newline: true)
p "a\r\nb".encode("UTF-8", universal_newline: true)
