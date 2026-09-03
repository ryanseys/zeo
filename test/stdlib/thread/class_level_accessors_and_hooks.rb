# Three things minitest's own bootstrap needs, none of which worked.
#
# `(class << self; self; end).attr_accessor :name` -- an attr defined on a
# SINGLETON class is a singleton attr on its owner, over that owner's ivars,
# not an instance method of a shared class. minitest's `cattr_accessor` is
# written exactly this way and every read raised NoMethodError.
module Config
  def self.cattr_accessor name
    (class << self; self; end).attr_accessor name
  end

  cattr_accessor :seed
  cattr_accessor :reporter
end

Config.seed = 42
p Config.seed
p Config.reporter
p Config.respond_to?(:seed=)

# The same on an ordinary object's singleton.
obj = Object.new
obj.singleton_class.attr_accessor :tag
obj.tag = "x"
p obj.tag
p Object.new.respond_to?(:tag)

# An `inherited` hook only sees subclasses defined AFTER it is installed.
# minitest reopens `Runnable` at the very END of its main file purely to add
# the hook, so that the `Test` and `Result` subclasses written above stay out
# of the registry -- fire it for those and `Result`, which implements no
# `runnable_methods`, joins the test run and raises.
class Base
  @@subs = []
  def self.subs = @@subs
end

class Early < Base
end

class Base
  def self.inherited klass
    @@subs << klass.name
    super
  end
end

class Late < Base
end

class Later < Late
end

p Base.subs

# `require "thread"` is a no-op CRuby keeps only so old code still loads --
# minitest/parallel.rb opens with it, and zeo used to raise LoadError. Only
# that it LOADS is asserted: CRuby answers false (the name is already in
# $LOADED_FEATURES before the program starts) where zeo answers true, the same
# way it already does for every other name `is_builtin_feature` satisfies.
require "thread"
p defined?(Thread)

# `Thread#abort_on_exception` and its process-wide default.
p Thread.abort_on_exception
t = Thread.new { 1 }
t.join
p t.abort_on_exception
t.abort_on_exception = true
p t.abort_on_exception
p Thread.current.abort_on_exception
__END__
42
nil
true
"x"
false
["Late", "Later"]
"constant"
false
false
true
false
