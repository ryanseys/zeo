# A method written at the TOP LEVEL lands on Object, and its receiver is
# the runtime `main` object -- name-keyed ivars, the same storage the
# top-level scope itself writes.
@config = "top"
def read_config
  x = 1
  @config
end
p read_config
def read2 = @config
p read2
def w(v)
  @config = v
  nil
end
w("changed")
p @config
p read_config
