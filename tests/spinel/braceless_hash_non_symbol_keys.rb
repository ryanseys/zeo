# A braceless hash argument is the keyword-argument spelling only when its keys
# are Symbols. A non-Symbol key cannot be a keyword at all, so Ruby passes an
# ordinary Hash positionally -- and pinning the collapsed parameter to the
# symbol-keyed variant regardless made the callee read a string-keyed hash
# through the wrong pointer: one empty-symbol key and no value, or a hang on
# integer and mixed keys (#3487).
#
# Each form needs its own method: giving one method both a braceless and a
# braced call site merges the inferred parameter type and hides the divergence.
def s_key(h)
  p h
end

def i_key(h)
  p h
end

def mixed(h)
  p h
end

def sym_key(h)
  p h
end

def braced(h)
  p h
end

def dflt(h = {})
  p h
end

def after_positional(a, h)
  p [a, h]
end

def reads_it(h)
  p h["m"]
end

s_key('m' => 1)
i_key(1 => 2)
mixed('a' => 1, :b => 2)
sym_key(k: 1)
braced({ 'm' => 1 })
dflt('z' => 9)
dflt
after_positional(5, 'q' => 7)
reads_it('m' => 42)

# the paths that already worked stay working
h = { "z" => 9 }
p h.merge('a' => 1)
p [].push('a' => 1)
f = ->(x) { x }
p f.call('a' => 1)

def rest(*a)
  p a
end
rest('a' => 1)
