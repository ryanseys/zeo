# Integer(s) raises ArgumentError on unparseable input, matching
# CRuby semantics. Previously spinel emitted bare strtoll(s, NULL, 10)
# which silently returned 0 for invalid input — that meant
# `Integer(s) rescue 0` always took the main branch and the rescue
# never fired.
#
# Test uses same-type rescue fallbacks (int + int) to avoid the
# int+string type-mismatch issue (a separate poly-boxing PR). For
# the bare-rescue path, e.g. `rescue ArgumentError => e`, we use
# `puts e` directly — exception .message dispatch lives on a
# separate exception-bindings PR.

# Valid inputs unchanged
a = (Integer("42") rescue -1)
puts a
b = (Integer("0") rescue -1)
puts b
c = (Integer("-7") rescue -1)
puts c
d = (Integer("+5") rescue -1)
puts d

# Whitespace handling per CRuby (leading/trailing whitespace ok).
# Includes \t \n \r \v \f — all should be skipped (isspace covers
# them; the previous manual `c == ' ' || c == '\t' || c == '\n' ||
# c == '\r'` check missed \v and \f).
e = (Integer("  -7  ") rescue -1)
puts e
e_tab = (Integer("\t42\t") rescue -1)
puts e_tab
e_nl = (Integer("\n42\n") rescue -1)
puts e_nl
e_cr = (Integer("\r42\r") rescue -1)
puts e_cr
e_vt = (Integer("\v42\v") rescue -1)
puts e_vt
e_ff = (Integer("\f42\f") rescue -1)
puts e_ff
e_mix = (Integer("  \t\n -7 \r\f ") rescue -1)
puts e_mix

# Sign handling
sign_pos_zero = (Integer("+0") rescue -1); puts sign_pos_zero
sign_neg_zero = (Integer("-0") rescue -1); puts sign_neg_zero
sign_plus_sp  = (Integer("+ 7") rescue -1); puts sign_plus_sp   # space between sign and digit → invalid
sign_minus_sp = (Integer("- 7") rescue -1); puts sign_minus_sp
sign_double   = (Integer("++7") rescue -1); puts sign_double    # double-sign → invalid
sign_bare     = (Integer("+")   rescue -1); puts sign_bare      # bare sign → invalid

# Edge magnitudes within mrb_int range
ll_max = (Integer("9223372036854775807")  rescue -1); puts ll_max   # LLONG_MAX
ll_min = (Integer("-9223372036854775808") rescue -1); puts ll_min   # LLONG_MIN

# Invalid inputs now raise (caught by rescue, returns -1)
f = (Integer("xyz") rescue -1); puts f
g = (Integer("") rescue -1); puts g
h = (Integer("   ") rescue -1); puts h
ii = (Integer("12abc") rescue -1); puts ii
j = (Integer("12.5") rescue -1); puts j

# Trailing-junk variants
trail_junk      = (Integer("42x") rescue -1);          puts trail_junk
trail_junk_ws   = (Integer("42 x") rescue -1);         puts trail_junk_ws       # whitespace then junk
trail_nl_junk   = (Integer("42\n garbage") rescue -1); puts trail_nl_junk
trail_only_ws   = (Integer("42 ") rescue -1);          puts trail_only_ws       # bare trailing whitespace OK

# Leading-junk variants
lead_junk    = (Integer("x42") rescue -1);    puts lead_junk
lead_ws_junk = (Integer(" x 42") rescue -1);  puts lead_ws_junk

# Hex prefixes ("0x10") and underscore separators ("12_345") are
# CRuby-supported but Spinel's base-10 strtoll path doesn't parse
# them. Documented in the PR's "Out of scope" — not exercised here
# because the test compares spinel output against MRI directly,
# and we'd diverge.

# Bare rescue catches and lets us see the error message.
# Also exercises the GC-root path: the message is allocated via
# sp_sprintf (str-heap) and stored in sp_exc_msg; reading it from
# `err` after a hypothetical mid-flight collection would dangle
# without sp_mark_in_flight_exceptions marking the slot.
begin
  Integer("nope")
rescue ArgumentError => err
  puts "msg: #{err}"
end

# Stress the GC-root fix: many raises in succession force allocator
# churn. Each `Integer("bad...")` builds an sp_sprintf-allocated
# message and stores it in sp_exc_msg before longjmping. Pre-fix
# (without sp_mark_in_flight_exceptions), a sweep landing between
# raise and rescue would reap the message; the rescue handler would
# read garbage. Post-fix, the marker keeps each message live until
# the handler returns and sp_exc_top decrements.
i = 0
ok = 0
while i < 100
  caught = 0
  begin
    Integer("bad")
  rescue ArgumentError
    caught = 1
  end
  ok += caught
  i += 1
end
puts "stress ok: #{ok}"

# Pre-existing endless_method_rescue.rb shape continues to work.
# Integer("abc") now raises but the rescue catches and returns 0,
# so the observed behaviour is unchanged.
def parse_int(s) = Integer(s) rescue 0
puts parse_int("42")
puts parse_int("abc")
puts parse_int("0")
puts parse_int("-7")
