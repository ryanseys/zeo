# Method#arity for a builtin-receiver method object comes from a table dumped
# from CRuby (#2700): the synthesized __bam wrapper's params are ABI plumbing,
# not the builtin's signature.
p("hello".method(:upcase).arity)
p(5.method(:+).arity)
p([1].method(:size).arity)
p("s".method(:gsub).arity)
p((1..2).method(:each).arity)
m = "hello".method(:upcase)
p m.arity
p m.call
# user methods keep the def-derived arity
class U
  def two(a, b); a + b; end
  def var(a, *rest); a; end
end
p U.new.method(:two).arity
p U.new.method(:var).arity
