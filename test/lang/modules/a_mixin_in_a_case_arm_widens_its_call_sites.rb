# A `prepend`/`include`/`extend` reachable through a class-body `case` arm is
# an ancestry edit exactly as one in an `if` branch is. Whether it happens is a
# runtime fact, so every call site the module could newly answer has to stop
# folding -- `C.new.tag` below must reach the prepended override, not the
# class's own body underneath it.
module Tagger
  def tag
    :from_module
  end
end

class Widened
  def tag
    :from_class
  end
end

class Widened
  case ENV.fetch("ZEO_MIXIN_MODE", "on")
  when "on" then prepend Tagger
  else nil
  end
end

p Widened.new.tag
p Widened.ancestors.first
__END__
:from_module
Tagger
