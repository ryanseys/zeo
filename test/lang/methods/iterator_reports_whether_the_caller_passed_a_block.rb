# Kernel#iterator? inside a method answers true only when called with a block,
# as a value and in a condition.
# (spinel issue #3251)
def has_block? = iterator?
p(has_block? { 1 })
p(has_block?)
def hb2
  iterator? ? "yes" : "no"
end
p(hb2 { })
p(hb2)
__END__
true
false
"yes"
"no"
