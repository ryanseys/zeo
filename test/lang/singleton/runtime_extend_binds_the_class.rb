# `Klass.extend M` written as a RUNTIME call, as opposed to `extend M` in the
# class body. Zeo ran the module's compiled body against a fresh blank
# instance of the class, one per call -- so `self` was not the class, an
# `@ivar` write went to a throwaway object and vanished, and an implicit-self
# call could only see instance methods. Nothing raised; the writes were simply
# lost.
#
# The singleton gem is built on this: `include Singleton` fires a hook that
# does `klass.extend SingletonClassMethods` and then
# `klass.instance_eval { set_mutex(Thread::Mutex.new) }`, and `Singleton#instance`
# then synchronizes on a mutex that stayed nil.
module Store
  def set_tag(v)
    @tag = v
  end

  def tag = @tag

  def build = new

  private

  def set_secret(v)
    @secret = v
  end

  def secret = @secret
end

class Compiled
  extend Store
end

class Runtime
end
Runtime.extend Store

# The class-body form already worked; the runtime form must agree with it.
Compiled.set_tag("compiled")
Runtime.set_tag("runtime")
p [Compiled.tag, Runtime.tag]
p [Compiled.instance_variable_get(:@tag), Runtime.instance_variable_get(:@tag)]

# ...through `send`, and through `instance_eval`, which is how the singleton
# gem reaches its private writers.
Runtime.send(:set_secret, "hidden")
p Runtime.send(:secret)
Runtime.instance_eval { set_tag("via-eval") }
p Runtime.tag

# `self` really is the class, so an implicit-self CLASS-method call resolves.
p Runtime.build.class
p Compiled.build.class

# A method taking the class as `self` through a plain call, too.
def init(klass)
  klass.instance_eval { set_tag("initialized") }
  klass
end
p init(Runtime).tag
__END__
["compiled", "runtime"]
["compiled", "runtime"]
"hidden"
"via-eval"
Runtime
Compiled
"initialized"
