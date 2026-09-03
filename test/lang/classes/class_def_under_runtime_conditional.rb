# A class or module body guarded by a RUNTIME condition.
#
# Gems that support more than one Ruby implementation define whole classes
# behind a platform probe. concurrent-ruby does exactly this, which is why
# `require "concurrent"` does not compile:
#
#   # concurrent-ruby/concurrent/executor/java_thread_pool_executor.rb:1
#   if Concurrent.on_jruby?
#     module Concurrent
#       class JavaThreadPoolExecutor < RubyThreadPoolExecutor
#   ...
#
# zeo resolves class shape at compile time, so it lowers a definition inside a
# top-level `if` only when the condition is compile-time decidable (such as
# `defined?(SomeConstant)`), or when the body only reopens an already-defined
# class to add methods. A method call is neither, so the compile stops.
#
# Ruby runs the false branch and prints "native".

def on_jruby?
  RUBY_PLATFORM == "java"
end

if on_jruby?
  class Backend
    def name
      "java"
    end
  end
else
  class Backend
    def name
      "native"
    end
  end
end

puts Backend.new.name
__END__
native
