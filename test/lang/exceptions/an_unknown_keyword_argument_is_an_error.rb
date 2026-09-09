# A keyword a method does not declare raises ArgumentError ("unknown
# keyword: :b") on EVERY call path. zeo's compile-time arity check gets the
# static shapes right; the DYNAMIC paths -- a runtime-defined method, a
# `Method#call`, a `send` -- silently drop the extra keyword and the call
# succeeds. Likely the `**kwrest`-peel shape from the kwargs channel.
c = Class.new { def kw(a:) = a }

begin
  p c.new.kw(a: 1, b: 2)
rescue ArgumentError => e
  puts e.message
end

m = c.new.method(:kw)
begin
  p m.call(a: 1, z: 9)
rescue ArgumentError => e
  puts e.message
end

begin
  p c.new.send(:kw, a: 1, q: 3)
rescue ArgumentError => e
  puts e.message
end
__END__
unknown keyword: :b
unknown keyword: :z
unknown keyword: :q
