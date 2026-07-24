# Bundled tests:
#   - str_nul_bound_slice_bytes
#   - str_poly_hash_merge

# === str_nul_bound_slice_bytes ===
def t_str_nul_bound_slice_bytes
  # Issue #657: a string with embedded NUL bytes had two surviving
  # truncation paths:
  #
  #   - s[a, n] slice walks the result range via sp_utf8_byte_offset
  #     applied to (s + boff, n). The inner call fell back to strlen
  #     on a mid-string pointer (no 0xfe/0xfc marker visible at [-1])
  #     and stopped at the first NUL after boff.
  #
  #   - s.bytes[i] (via sp_str_bytes) used `for (i=0; s[i]; i++)` which
  #     terminates at the first NUL byte.
  #
  # Fix:
  #   - sp_utf8_byte_offset bounds the walk on sp_str_byte_len(s)
  #     (which honours the heap-string hdr marker) instead of "*p".
  #   - sp_str_sub_range / sp_str_sub_range_len compute the end byte
  #     position inline against sp_str_byte_len(s) instead of recursing
  #     into sp_utf8_byte_offset(s+boff, len).
  #   - sp_str_bytes walks 0..sp_str_byte_len(s) instead of 0..NUL.
  #
  # Concat already round-trips NULs correctly (sp_str_concat uses
  # sp_str_byte_len for both inputs).
  
  nul = 0.chr
  s = "abc" + nul + "def"
  puts "len=#{s.length}"
  puts "slice_full=#{s[0, 7].length}"
  puts "slice_after_nul=#{s[4, 3].length}"
  puts "slice_around_nul=#{s[2, 3].length}"
  puts "b0=#{s.bytes[0]}"
  puts "b3=#{s.bytes[3]}"
  puts "b4=#{s.bytes[4]}"
  puts "b6=#{s.bytes[6]}"
  puts "bytes_len=#{s.bytes.length}"
end
t_str_nul_bound_slice_bytes

# === str_poly_hash_merge ===
def t_str_poly_hash_merge
  # Issue #426. `Hash#merge` was unresolved on a `str_poly_hash`
  # (mixed-value-type hash literal) -- emit produced `0` and the
  # downstream `m.length` cascaded through int. Same shape on
  # monomorphic-value hashes (`str_int_hash`, `str_str_hash`)
  # worked; the gap was just the polymorphic-value specialization.
  #
  # Fix:
  #   - Runtime: sp_StrPolyHash_merge(a, b) in lib/sp_runtime.h,
  #     same shape as the existing sp_StrIntHash_merge /
  #     sp_SymPolyHash_merge helpers.
  #   - Codegen: `compile_hash_method_expr`'s str_poly_hash arm
  #     gains a `merge` case. Dispatches by arg type:
  #       * str_poly_hash arg -> sp_StrPolyHash_merge direct.
  #       * str_str_hash arg -> inline copy with sp_box_str on
  #         each value to land in poly slots.
  #       * str_int_hash arg -> inline copy with sp_box_int.
  #
  # Out of scope: poly-typed args (sp_RbVal whose runtime hash
  # variant is unknown statically) -- needs the runtime-dispatch
  # pattern from sym_poly_hash's poly-arg branch, not in this
  # minimal fix.
  #
  # Coverage:
  #   - canonical mixed-value seed + str_int_hash override (the
  #     repro from #426).
  #   - mixed-value seed + str_str_hash override (forces the
  #     box_str copy path).
  #   - mixed-value seed + str_poly_hash override (the direct
  #     merge path; verifies it's still reachable post-fix).
  
  h = { "a" => 1, "b" => "two" }  # str_poly_hash
  m = h.merge({ "c" => 3 })       # str_int_hash arg
  puts m.length                    # 3
  puts m["a"] == 1 ? "a-ok" : "a-bad"
  puts m["c"] == 3 ? "c-ok" : "c-bad"
  
  # str_str_hash override of a str_poly_hash recv.
  h2 = { "x" => 1, "y" => "two" }
  m2 = h2.merge({ "z" => "three" })
  puts m2.length                   # 3
  puts m2["z"] == "three" ? "z-ok" : "z-bad"
  
  # str_poly_hash override of a str_poly_hash recv.
  h3 = { "p" => 1, "q" => "two" }
  extra = { "r" => 3, "s" => "four" }
  m3 = h3.merge(extra)
  puts m3.length                   # 4
end
t_str_poly_hash_merge

