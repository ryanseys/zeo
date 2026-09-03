# Ruby's keywords-to-positional-hash conversion, still true in 3+: with
# no keyword params and no `**kwrest` declared, trailing keywords are
# not keywords at all -- they are one positional Hash. This is what
# makes the classic options-hash idiom work, and it was raising
# `unknown keyword: :a` for a method that has no keywords to be
# unknown. A callee that DOES declare keywords keeps the real check.

def m(opts = {}); opts; end
p m(a: 1, b: 2)
p m({a: 1})
p m
class C
  def initialize(opts = {}); @o = opts; end
  attr_reader :o
end
p C.new(a: 1).o
def k(x:); x; end
begin
  k(x: 1, zz: 2)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
__END__
{a: 1, b: 2}
{a: 1}
{}
{a: 1}
ArgumentError: unknown keyword: :zz
