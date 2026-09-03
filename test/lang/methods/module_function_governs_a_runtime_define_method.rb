# A bare `module_function` is a MODE: every definition after it in the body
# becomes both a module method and a private instance copy. Zeo resolves
# that cursor at compile time, which reaches a `def` and reaches the block
# form of `define_method` (it desugars to a `def`) -- but a
# `define_method(:name, <a callable>)` installs at RUN time, where the
# compile-time cursor cannot follow it.
#
# This is rack's shape, and the reason `Rack::Utils.escape_html` was
# missing: `define_method(:escape_html, ERB::Escape.instance_method(:html_escape))`
# under a bare `module_function`.
module Src
  def html_escape(s) = "esc(#{s})"
end
shout = ->(s) { "lam(#{s})" }

module U
  module_function

  def plain(s) = "plain(#{s})"

  define_method(:blk) { |s| "blk(#{s})" }
  define_method(:unbound, Src.instance_method(:html_escape))
  define_method(:lambda_arg, ->(s) { "lam(#{s})" })
  define_method("string_name", ->(s) { "str(#{s})" })

  # rack's own shape: the definition sits in a BRANCH of the module body,
  # so the mode has to reach it there too.
  if defined?(Src) && Src.instance_method(:html_escape)
    define_method(:in_a_branch, Src.instance_method(:html_escape))
  else
    def in_a_branch(s) = "fallback(#{s})"
  end
end

%i[plain blk unbound lambda_arg string_name in_a_branch].each do |m|
  puts U.public_send(m, "x")
end

# Both halves, in ruby's own reporting: the module method is public, the
# instance copy private.
p U.instance_methods(false).sort
p U.private_instance_methods(false).sort
p U.singleton_methods(false).sort

# The instance copy is what an `include` mixes in, private and all.
class Host
  include U
  def use(s) = unbound(s)
end
puts Host.new.use("y")
begin
  Host.new.unbound("z")
rescue NoMethodError => e
  puts e.message
end

# Without the mode, a run-time define_method stays a public instance
# method and nothing is promoted.
module Plain
  define_method(:only_instance, ->(s) { s })
end
p Plain.instance_methods(false)
p Plain.singleton_methods(false)
__END__
plain(x)
blk(x)
esc(x)
lam(x)
str(x)
esc(x)
[]
[:blk, :in_a_branch, :lambda_arg, :plain, :string_name, :unbound]
[:blk, :in_a_branch, :lambda_arg, :plain, :string_name, :unbound]
esc(y)
private method 'unbound' called for an instance of Host
[:only_instance]
[]
