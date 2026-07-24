# Tail-position `if x.is_a?(C) ... end` now pushes a narrow for `x`
# inside the body, so a typed callee that consumes `x` unboxes via
# the matching union field (`.v.s` for string, `.v.i` for int, etc.)
# instead of emitting the raw sp_RbVal. Issue #612: pre-fix
# Regexp.escape on a poly param narrowed via `is_a?(String)` emitted
# `sp_re_escape(lv_x)` against the un-narrowed slot and the C
# compile failed.

def matcher(content_or_opts)
  if content_or_opts.is_a?(Hash)
    content = nil
  else
    content = content_or_opts
  end
  if content.is_a?(String)
    pattern = Regexp.new("<h1>#{Regexp.escape(content)}<")
    body = "<h1>Title<"
    puts pattern.match?(body) ? "match" : "no match"
  end
end

matcher("Title")
matcher({ "opt" => 1 })   # content is nil, skips the inner if entirely
