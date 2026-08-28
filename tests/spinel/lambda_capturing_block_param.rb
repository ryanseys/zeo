# A lambda's required parameters are typed from the call sites the analysis can
# see, and fall back to Integer when it sees none. That fallback is only safe
# for a lambda it can enumerate the callers of, so one that ESCAPES widens to
# poly instead -- including one that is a block's TAIL VALUE, which a collecting
# iterator boxes into its result array.
#
# desugar_block_capture_wrap rewrites a block whose parameter is captured into
# `->(__cap){ <body> }.call(param)`, so the block's tail becomes that call and
# the literal sits one level in. Reading the block's tail alone found the call,
# never marked the literal, and its parameter took the Integer default -- so the
# Proc read back out of the array bound an address-shaped number (#4064).
matchers = ["alpha"].map { |word| ->(line) { line.include?(word) } }
p matchers[0].call("an alpha line")

pairs = ["alpha"].map { |word| ->(line) { [line, word] } }
p pairs[0].call("a line")

# the same shape without map in it: a method that yields and returns the value
def first_of(list)
  list.each { |k| return yield(k) }
end
f = first_of(["beta"]) { |word| ->(line) { line.include?(word) } }
p f.call("a beta line")
p f.call("nothing here")

# capturing a block-local rather than the parameter still works
locals = ["gamma"].map { |word| w = "#{word}"; ->(line) { line.include?(w) } }
p locals[0].call("a gamma line")

# each closure keeps its OWN copy of the captured parameter
all = %w[a b c].map { |ch| ->(s) { s + ch } }
p all.map { |l| l.call("x") }
