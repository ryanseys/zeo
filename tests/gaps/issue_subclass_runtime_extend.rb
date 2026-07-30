# A subclass does not inherit class methods installed by a RUNTIME `extend`.
# `runtime_extend` writes the methods into the extended class's own overlay
# `class_methods`, and a call on the subclass probes only the subclass's
# overlay -- it never walks its ancestors' overlays. CRuby resolves the call
# through the parent's singleton chain, which the subclass's singleton class
# inherits.
#
# The class-body form (`class Base; extend Store; end`) already works and is
# covered by `tests/runtime_extend_binds_the_class.rb`; this case is trimmed
# out of that golden.
module Store
  def tag = "tagged"
end

class Base
end
Base.extend Store

class Sub < Base
end

p Base.tag
p Sub.tag
