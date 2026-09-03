# `foo(1, **h)` with an empty `h` passes just `1` -- the trailing hash
# is pushed only when non-empty. This is about a RUNTIME-empty hash, not
# the literal `**{}`, so it can't be decided at lowering time. Pushing
# unconditionally silently handed the callee an extra `{}`.

def foo(*z); z; end
def c(h); foo(1, **h); end
p c({})
p c({k: 2})

def kw(*z); z; end
def d(h); kw(**h); end
p d({})

def both(h); foo(1, k: 1, **h); end
p both({})
p both({j: 2})
__END__
[1]
[1, {k: 2}]
[]
[1, {k: 1}]
[1, {k: 1, j: 2}]
