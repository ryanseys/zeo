# `super` inside a `def` in a `class << SOME_VALUE` body raises
# "super called outside of method". The method itself installs and dispatches
# correctly -- only `super` is wrong -- so the body is reaching the runtime as
# a BLOCK rather than as a method.
#
# `desugar_singleton_items` rebinds a `class << obj` body onto a synthesized
# `obj.singleton_class`, which is the per-object path (the `class << self`
# path is `map_class_self_items`, and a plain `def` on either works -- see
# `an_alias_of_a_builtin_binds_the_body.rb` and this file's own second case).
# What the rebind does not carry is the frame kind, so `super` has no method
# entry to walk up from.
#
# Found while running `extconf.rb` under zeo for the C extension work.
# `lib/mkmf.rb` opens with
#
#     STRING_OR_FAILED_FORMAT = "%s"
#     class << STRING_OR_FAILED_FORMAT
#       def %(x) = x ? super : "failed"
#     end
#
# which is the shape below, and it is the first thing that stops mkmf from
# loading. So this gap blocks the whole C-extension build pipeline, not just
# a corner of reflection.

module M
  FMT = "%s"
  class << FMT
    def %(other)
      other ? super : "failed"
    end
  end
end

p M::FMT % "ok"
p M::FMT % nil

# The control: the same singleton def with no `super` already works, which is
# what narrows this to the frame kind rather than the rebind.
module N
  TAG = "tag"
  class << TAG
    def shout = upcase
  end
end
p N::TAG.shout
__END__
"ok"
"failed"
"TAG"
