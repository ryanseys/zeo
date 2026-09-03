# `@x` in a class body/class method and `@x` in an instance method name
# two completely different slots -- the class object's own, and the
# instance's. Also exercises the same-name collision across Ruby's two
# method namespaces (`def self.x` + `def x`), which share one generated
# container and so need `ident::class_method_ident`'s mangling.

class C
  @x = "class-level"
  def initialize; @x = "instance-level"; end
  def self.x; @x; end
  def x; @x; end
end
p [C.x, C.new.x]
__END__
["class-level", "instance-level"]
