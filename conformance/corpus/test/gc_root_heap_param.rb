# A heap-typed (poly / string) method parameter must stay GC-rooted
# across a collection triggered inside the method, so a later read of it
# (e.g. sp_poly_to_s in a %s interpolation arg) is not a use-after-free.
# Regression for #1068 (follow-up to #1057).
def gc_then
  GC.start
  i = 0
  s = ""
  while i < 6000
    s = s + i.to_s + ","
    i += 1
  end
  "G"
end

def wrap(body)
  "#{gc_then}-#{body}"
end

wrap(42)                  # int call site forces `body` to a poly param
puts wrap("P_" + 7.to_s)  # string call site
