# A method installed by `module_eval` on a STRING can reach the module's other
# methods through `self`. i18n's delegators do exactly that -- the eval'd
# `backend=` calls `writable_config`, a literal `def` two lines above it.
#
# The seam is `extend`: an extended module's method has to run with the CLASS
# as `self`. A compiled body needs an object receiver and gets a surrogate
# whose class IS the extending class, and a runtime-defined body took that
# same route -- so `self` read as an instance of Host and every sibling call
# missed. A runtime body is already value-shaped and takes `self` directly.
module Host
  module Base
    def helper = :helper

    %w(alpha).each do |name|
      module_eval <<-RUBY, __FILE__, __LINE__ + 1
        def #{name}
          helper
        end
      RUBY
    end
  end
  extend Base
end

p Host.alpha

class Direct
  include Host::Base
end
p Direct.new.alpha

# `self` is the module itself, not an instance of it, and the ivars it writes
# are the extending object's own.
module Wide
  module Api
    def helper = :helper
    def ivar_write; @stash = 7; end
    module_eval "def evald; [self.to_s, self.class.to_s, helper]; end"
    define_method(:defined_m) { [self.to_s, helper] }
  end
  extend Api
end
p Wide.evald
p Wide.defined_m
Wide.ivar_write
p Wide.instance_variable_get(:@stash)
p Wide.singleton_methods(false)
p Wide.singleton_methods.sort
p [Wide.method(:evald).owner.to_s, Wide.respond_to?(:evald)]

# A class and a plain object extend the same way.
class Klass
  extend Wide::Api
end
p Klass.evald
o = Object.new
o.extend(Wide::Api)
p o.evald[1..]
__END__
:helper
:helper
["Wide", "Module", :helper]
["Wide", :helper]
7
[]
[:defined_m, :evald, :helper, :ivar_write]
["Wide::Api", true]
["Klass", "Class", :helper]
["Object", :helper]
