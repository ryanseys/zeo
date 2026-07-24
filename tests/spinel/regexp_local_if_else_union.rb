# An if/else expression whose branches both build a Regexp now
# unifies to the new `regexp` type (mrb_regexp_pattern *) instead
# of collapsing to int. Before this fix Regexp.new(...) was
# untyped (default int fallback), the if/else result was int, and
# the resulting `pattern.match?(...)` call had no statically
# resolvable pattern to thread through — the codegen warned
# "cannot resolve call to 'match?' on int (emitting 0)" and the
# binary always reported no match. Issue #609.

def check(use_a)
  pattern = if use_a
              Regexp.new("aaa")
            else
              Regexp.new("bbb")
            end
  puts pattern.match?("xaaay") ? "match" : "no match"
end

check(true)    # match    (uses /aaa/)
check(false)   # no match (uses /bbb/, body has no "bbb")

# Dynamic-source arm: each Regexp.new(<expr>) goes through the
# per-call-site sp_re_dyn_<i> cache, but the regexp-typed local
# still holds the resulting pointer.
def check_dyn(prefix, body)
  pattern = if prefix == "x"
              Regexp.new(prefix + "aaa")
            else
              Regexp.new(prefix + "bbb")
            end
  puts pattern.match?(body) ? "dyn-match" : "dyn-no-match"
end

check_dyn("x", "xaaa-yes")     # dyn-match
check_dyn("y", "ybbb-yes")     # dyn-match
check_dyn("x", "nope")         # dyn-no-match
