# A `tap` / `then` block parameter is another name for the call's own receiver,
# so a key operation written through it says the same thing about the literal
# that one written directly on it does. The rule that gives a bare `{}` the
# variant its key selects did not follow the parameter back, so the literal
# kept the String-keyed default and an Array key went into a `const char *`
# slot (#4026):
#
#   p( {}.tap{ |h| h[[]] } )
#   # error: incompatible pointer types passing 'sp_IntArray *' to
#   #        parameter of type 'const char *'
p({}.tap { |h| h[[]] })
p({}.then { |h| h[[]] })
p({}.tap { |h| h[[1]] })
p({}.tap { |h| h[1.5] })
p({}.tap { |h| h[:a] })
p({}.tap { |h| h["k"] })

# the WRITE forms name their key in the same place, and were not in the rule's
# name list at all -- so a Symbol key through tap built a String-keyed hash
p({}.tap { |h| h[:a] = 1 })
p({}.tap { |h| h["k"] = 1 })
p({}.tap { |h| h[1] = 2 })
p({}.tap { |h| h[[1, 2]] = 3 })
p({}.tap { |h| h.store([1], 2) })
p({}.tap { |h| h.store(:s, 4) })

# nested, and reaching the same literal twice
p({}.tap { |h| h[[1]] = 1 }.tap { |g| g[[2]] = 2 })

# the direct spellings keep answering the same thing
h = {}
h[:a] = 1
p h
g = {}
g[[1]] = 2
p g
p({}.fetch([], "d"))
