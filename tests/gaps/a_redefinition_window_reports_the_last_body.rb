# Inside a REDEFINITION WINDOW -- between two same-name `def`s -- zeo runs the
# right body but REPORTS the last one, and does not enforce a visibility mark
# made before the reopen. Both halves are true on BOTH channels.
#
# `analyze::redefs` puts the timeline in the runtime overlay: the first body
# installs at boot and each later one re-installs at its own document position,
# so dispatch is correct (`tests/class_method_redefinition_timeline.rb` and
# `tests/a_later_def_on_a_module_installs_at_its_line.rb` sweep it). What stays
# static is everything BESIDE the body:
#
#   * the META rows -- `#arity`, `#parameters`, `#source_location` -- are
#     emitted once, for the final body, because they are compile-time tables
#     keyed by `(class, name)` with no position;
#   * a visibility mark is a positional statement in its own right, and the
#     boot install writes the overlay row without one, so a `private`/
#     `private_class_method` made before the reopen is not enforced in the
#     window.
#
# Fixing either means giving the overlay row the metadata the static tables
# carry: a `MetaRow` per POSITION rather than per name, and the visibility
# verb applied to the overlay row rather than to the frozen one. That is a
# widening of the mechanism, not a bug in it -- so it is filed rather than
# patched at one of its four install sites.
#
# Every line below runs the correct BODY; only the reports differ.

class R
  def r = "r1"
end
p [R.instance_method(:r).arity, R.new.r]
class R
  def r(x) = "r2"
end
p [R.instance_method(:r).arity, R.new.r(1)]

class CR
  def self.r = "cr1"
end
p [CR.method(:r).arity, CR.r]
class CR
  def self.r(x) = "cr2"
end
p [CR.method(:r).arity, CR.r(1)]

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
