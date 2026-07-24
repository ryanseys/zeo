def has_block? = iterator?
p(has_block? { 1 })
p(has_block?)
def hb2
  iterator? ? "yes" : "no"
end
p(hb2 { })
p(hb2)
