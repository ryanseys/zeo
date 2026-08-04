# A bare `k: v` argument degrades to one positional hash at a `*rest`
# parameter's tail. One call path did that and the other dropped it entirely,
# so an INSTANCE method ran with no arguments at all while the identical
# top-level call kept them -- silently, no error (#3503).
class C
  def splat_only(*parts) = parts.length
  def leading_arg(a, *parts) = parts.length
  def kwrest(**kw) = kw.length
  def named_kw(id:) = id.to_s
  def rest_and_named(*parts, id:) = [parts.length, id.to_s]
  def contents(*parts) = parts
end

def top(*parts) = parts.length

c = C.new
p top(id: :desc)
p c.splat_only(id: :desc)
p c.splat_only({ id: :desc })
p c.leading_arg(1, id: :desc)
p c.kwrest(id: :desc)
p c.splat_only
p c.splat_only(1, 2)
p c.splat_only(1, id: :desc)
p c.splat_only("a" => 1)

# a named keyword param still consumes the hash rather than seeing it as rest
p c.named_kw(id: :desc)
p c.rest_and_named(1, 2, id: :desc)

# the hash that lands in rest is the real thing
p c.contents(id: :desc)
p c.contents(1, id: :desc)
