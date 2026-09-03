# The ClassMethods idiom where the `include` sits in a REOPEN, and where the
# hook is inherited one class further down.
#
# Both are the shape bundler actually has, and both were missed after the
# first cut of the fix: `analyze::classes` takes a different branch for a
# reopen site (`defer_positional_mixin`, which stages the mixin as a runtime
# send) and that branch skipped recording the edge. `class Bundler::Thor;
# include Bundler::Thor::Base` IS a reopen, because the nested `Thor::*`
# files create the shell first -- so the whole of Thor missed it.
#
# The lesson worth keeping: an edge belongs to the INCLUDE, not to how the
# mixin happens to be staged.
module CM
  def method_added(name) = ((@added ||= []) << name)
  def added = @added
end

module Base
  class << self
    def included(base) = base.extend(CM)
  end
end

class Reopened; end

class Reopened
  include Base
  def a; end
end

p Reopened.added

# One further down: the hook arrives on the superclass and the subclass's own
# definitions must still announce.
class Below < Reopened
  def b; end
  def c; end
end

p Below.added
__END__
[:a]
[:b, :c]
