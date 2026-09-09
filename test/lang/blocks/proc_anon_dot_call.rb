# `proc { ... }.call` where the receiver is the literal proc rather than a
# local holding it: the bare literal, one taking arguments, one returned by a
# method, and one chosen by a ternary.

def show(s)
  puts s
end

# Anonymous proc literal — the original bug.
proc { show("anon") }.call
proc { show("second") }.call
proc { |n| show((n * 2).to_s) }.call(7)

# CallNode receiver returning a proc (factory pattern).
def factory
  proc { show("factory") }
end
factory.call

# Ternary returning a proc — both branches must unify to `proc`.
cond = true
(cond ? proc { show("ternary-true") } : proc { show("ternary-false") }).call
__END__
anon
second
14
factory
ternary-true
