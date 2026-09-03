# `define_method(name, SomeModule.instance_method(:m))` -- a MODULE-owned
# UnboundMethod binds into any class (CRuby's rule; a CLASS-owned one still
# requires a descendant). rack installs ERB::Escape#html_escape into
# Rack::Utils this way without ever including the module.
module M
  def hi = "from-M(#{self.class})"
end
class C
  define_method(:hi2, M.instance_method(:hi))
end
p C.new.hi2
__END__
"from-M(C)"
