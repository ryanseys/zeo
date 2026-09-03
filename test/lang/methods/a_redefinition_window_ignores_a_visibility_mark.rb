# A `private` mark made before a reopen is enforced in the REDEFINITION
# WINDOW, and the reopen clears it -- both channels.
#
# The gap filed this as TWO writes: a mark to apply, and each install to
# rewrite it. It is ONE, because the fact was already in the right place.
# `lower::defs` retags a `def` IN PLACE when `private :v` names a method the
# same body just defined (there is no `MethodVisibility` node at all in that
# shape). So by the time `analyze::redefs` builds a timeline, every body
# already carries its own visibility, and installing it beside the body gives
# both halves at once: the first body's `private` applies, and the second
# body's `public` -- the class body's running default at that `def` -- is
# what clears it.
#
# That is also ruby's own rule. A re-`def` resets a name to the running
# default; it does not inherit the previous body's mark.
#
# The mark rides the existing install in both positions: bits 1-2 of
# `REG_BOOT_REDEF`'s flag byte for the first body (bit 0 was already the
# channel), and a `zeo_rt_install_positional_visibility` call beside each
# later one. That entry is infallible on purpose -- unlike
# `zeo_rt_runtime_set_visibility` there is no name to resolve and no
# `method_added` to fire, because the install on the line above created the
# method.
#
# Falling back to the static row would NOT have worked: it records the END
# state, which is public here.
class V
  def v = "v1"
  private :v
end
p [(V.new.v rescue $!.class.to_s), V.new.send(:v)]
class V
  def v = "v2"
end
p V.new.v

class CV
  def self.v = "cv1"
  private_class_method :v
end
p [(CV.v rescue $!.class.to_s), CV.send(:v)]
class CV
  def self.v = "cv2"
end
p CV.v

# The sweep. Each shape below is one a fix could plausibly get wrong.

# `protected` is the third mark, and it rides the same two bits.
class PR
  def m = "pr1"
  protected :m
end
p [PR.protected_instance_methods(false), (PR.new.m rescue $!.class.to_s)]
class PR
  def m = "pr2"
end
p [PR.protected_instance_methods(false), PR.new.m]

# The running DEFAULT, rather than a named mark: a bare `private` before the
# `def` stamps the body the same way.
class BD
  private
  def m = "bd1"
end
p [BD.private_instance_methods(false), (BD.new.m rescue $!.class.to_s)]
class BD
  def m = "bd2"
end
p [BD.private_instance_methods(false), BD.new.m]

# Three bodies, so a MIDDLE install has to clear and re-mark.
class TH
  def m = "t1"
  private :m
end
p [(TH.new.m rescue $!.class.to_s), TH.new.send(:m)]
class TH
  def m = "t2"
end
p TH.new.m
class TH
  def m = "t3"
  private :m
end
p [(TH.new.m rescue $!.class.to_s), TH.new.send(:m)]

# The reverse order: public first, private last. The static row says private,
# so the WINDOW is what a fix has to keep public.
class RV
  def m = "rv1"
end
p RV.new.m
class RV
  private def m = "rv2"
end
p [(RV.new.m rescue $!.class.to_s), RV.new.send(:m)]

# A mark written in a LATER body, naming a method an EARLIER body defined --
# the deferred `MethodVisibility` path rather than the in-place retag.
class LT
  def m = "lt1"
end
p LT.new.m
class LT
  private :m
end
p [(LT.new.m rescue $!.class.to_s), LT.new.send(:m)]

# A method with no redefinition at all keeps the ordinary static path.
class ST
  def a = "a"
  def b = "b"
  private :b
end
p [ST.new.a, (ST.new.b rescue $!.class.to_s), ST.private_instance_methods(false)]
__END__
["NoMethodError", "v1"]
"v2"
["NoMethodError", "cv1"]
"cv2"
[[:m], "NoMethodError"]
[[], "pr2"]
[[:m], "NoMethodError"]
[[], "bd2"]
["NoMethodError", "t1"]
"t2"
["NoMethodError", "t3"]
"rv1"
["NoMethodError", "rv2"]
"lt1"
["NoMethodError", "lt1"]
["a", "NoMethodError", [:b]]
