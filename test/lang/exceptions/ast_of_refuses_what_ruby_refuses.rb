# `RubyVM::AbstractSyntaxTree.of` refuses the same way CRuby does.
#
# Every ISEQ ruby builds is prism-compiled, so CRuby answers a RuntimeError
# naming that rather than an AST -- and the refusal is a CONTRACT, not an
# absence: `error_highlight` rescues it BY MESSAGE and reparses with prism
# itself (`ErrorHighlight.prism_find`). zeo answered `nil` for a `Method` and
# for a `Thread::Backtrace::Location`, which looked harmless and silently
# turned every spot off -- the rescue never fired, so the caller got no node
# and no error either.
#
# An `UnboundMethod` really is nil in CRuby, and a value that is not a
# callable at all is a TypeError naming its class.
def m = 1

def described(x)
  [x.class, RubyVM::AbstractSyntaxTree.of(x)]
rescue => e
  [x.class, :raised, e.class, e.message]
end

p described(method(:m))
p described(proc { 1 })
p described(lambda { 2 })
p described(Integer.instance_method(:to_s))
p described(42)
p described("not a callable")

begin
  nil.nope
rescue NoMethodError => e
  p described(e.backtrace_locations.first)
end
__END__
[Method, :raised, RuntimeError, "cannot get AST for ISEQ compiled by prism"]
[Proc, :raised, RuntimeError, "cannot get AST for ISEQ compiled by prism"]
[Proc, :raised, RuntimeError, "cannot get AST for ISEQ compiled by prism"]
[UnboundMethod, nil]
[Integer, :raised, TypeError, "wrong argument type Integer (expected method)"]
[String, :raised, TypeError, "wrong argument type String (expected method)"]
[Thread::Backtrace::Location, :raised, RuntimeError, "cannot get AST for ISEQ compiled by prism"]
