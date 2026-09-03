# `recv&.m` where a `using` covers `m`. The nil test is the only difference
# from an ordinary refined call: a nil receiver answers nil and evaluates
# neither the arguments nor the block.
module R
  refine String do
    def shout = upcase
    private def quiet = downcase
  end
end
module R2
  refine Array do
    def pick(n) = self[n]
    def each_twice(&b) = (each(&b); each(&b))
  end
end
using R
using R2

s = "hi"
p s&.shout
p nil&.shout
p [s, nil].map { |x| x&.shout }
begin
  s&.quiet
rescue NoMethodError => e
  puts e.message
end

a = [1, 2]
p a&.pick(1)
out = []
a&.each_twice { |x| out << x }
p out
p nil&.pick(0)

# The receiver is evaluated ONCE, and not at all past the nil test.
calls = 0
recv = ->(v) { calls += 1; v }
p recv.call(nil)&.shout
p recv.call("ok")&.shout
p calls

# A splat and a double-splat at a refined call: the element count is a
# run-time number, so the arguments travel as an Array.
module R3
  refine Hash do
    def merged(**kw) = merge(kw)
    def pick(*ns, **kw) = [ns, kw]
  end
end
using R3
h = { a: 1 }
extra = { b: 2 }
p h.merged(**extra)
p h.pick(*[1, 2], **extra)
p h.merged(c: 3)
p h&.pick(*[0])
__END__
"HI"
nil
["HI", nil]
private method 'quiet' called for an instance of String
2
[1, 2, 1, 2]
nil
nil
"OK"
2
{a: 1, b: 2}
[[1, 2], {b: 2}]
{a: 1, c: 3}
[[0], {}]
