# A runtime `undef_method` on a MODULE retires the name for every includer.
# zeo materializes an included method onto each includer's own flattened
# table, so the tombstone -- which sits on the module -- was somewhere the
# flattened probe never looked, and the includer went on answering.
module Retiring
  def self.retire = undef_method(:doomed)

  def doomed = "still here"
  def kept = "kept"
end

class UsesRetiring
  include Retiring
end

class AlsoUses
  include Retiring
  def doomed = "my own"
end

p UsesRetiring.new.doomed
p AlsoUses.new.doomed
Retiring.retire
begin
  UsesRetiring.new.doomed
rescue NoMethodError => e
  p e.class
end
# A NEARER own definition still wins -- the tombstone is farther up the chain.
p AlsoUses.new.doomed
# Everything else the module gave is untouched.
p UsesRetiring.new.kept
p UsesRetiring.new.respond_to?(:doomed)
p UsesRetiring.instance_method(:kept).owner.to_s

# A subclass of an includer sees it too.
class Deeper < UsesRetiring; end
begin
  Deeper.new.doomed
rescue NoMethodError => e
  p [:deeper, e.class]
end
