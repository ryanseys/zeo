# A `private` mark made before a reopen is not enforced in the REDEFINITION
# WINDOW. Both channels.
#
# The body half and the reflection half of a redefinition timeline are both
# positional now: `analyze::redefs` re-installs the body at each `def`'s
# document position, and `ProgramDesc::redef_metas` installs that body's
# `#arity`/`#parameters`/`#source_location` beside it
# (tests/a_redefinition_window_reports_its_own_body.rb). Visibility is the
# third thing a `def` carries, and it is still static.
#
# What makes it a third mechanism rather than a third field: a visibility mark
# is a STATEMENT of its own, not part of the `def`. `private :v` runs between
# the two bodies, and the LAST `def` resets the name to public -- which is why
# zeo's static table already answers the end state correctly and only the
# window is wrong.
#
# The overlay has the map to hold it (`OverlayEntry::methods_vis`), and the
# dispatch already prefers it. Two writes are missing and both are positional:
#   * each install writes the visibility the `def` at THAT position has (the
#     class body's running default), clearing any earlier mark;
#   * a `private`/`private_class_method` naming a redefined method emits a
#     runtime `zeo_rt_runtime_set_visibility` at its own line instead of only
#     a static `VisRow`.
#
# Falling back to the static row when the overlay carries no mark is NOT the
# fix -- the static row records the END state, which is public here.
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
