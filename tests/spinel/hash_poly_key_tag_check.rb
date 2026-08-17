# A hash lookup whose key arrives boxed must check the tag before reading the
# union member: the same call site reached with a String and with a Float read
# the Float's bits as a pointer (#3810).
NAMED = { 'red' => '#FF0000' }.freeze
def named?(v)
  NAMED.key?(v)
end
def fetch_it(v)
  NAMED[v]
end
p named?('red')
p named?(0.5)
p named?(:red)
p fetch_it('red')
p fetch_it(2)

SYMS = { a: 1 }
def sym_key?(v)
  SYMS.key?(v)
end
p sym_key?(:a)
p sym_key?('a')
p sym_key?(3.5)

INTS = { 7 => 'seven' }
def int_key?(v)
  INTS.key?(v)
end
p int_key?(7)
p int_key?('x')
