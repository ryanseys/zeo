# Issue #126 Stage 1: a module value assigned to a module-level
# `class << self; attr_accessor :x; end` accessor used to read back
# as 0. The whole `module ... class << self; ... end ... end`
# block was silently dropped (no `PM_SINGLETON_CLASS_NODE` parser
# support), and the entire bare identifier path for module names as
# rvalues didn't exist.
#
# Stage 1 handles the static-fold case: a single assignment of a
# constant-resolvable RHS (typically a module/class name) is folded
# at codegen, and read sites substitute the resolved constant in a
# `<recv>.<method>` chain. Polymorphic / multi-write slots fall
# through to the un-folded path (Stage 2).

# 1. Module assigned to a module-level singleton accessor — the
#    issue's exact reproducer.
module SqliteAdapter
  def self.answer
    42
  end
end

module ActiveRecord
  class << self
    attr_accessor :adapter
  end
end

ActiveRecord.adapter = SqliteAdapter
puts ActiveRecord.adapter.answer    # 42

# 2. The string-returning downstream method that surfaced the type
#    mismatch in the issue. Verifies the chain return type
#    threads through correctly (string, not int).
module Greeter
  def self.greet
    "hello"
  end
end

module Service
  class << self
    attr_accessor :greeter
  end
end

Service.greeter = Greeter
puts Service.greeter.greet          # hello

# 3. Class assigned to a module-level singleton accessor — same
#    constant-fold path applies to user-defined classes as long as
#    the reachable methods on the resolved name are class methods.
class CounterClass
  def self.count
    7
  end
end

module Holder
  class << self
    attr_accessor :counter
  end
end

Holder.counter = CounterClass
puts Holder.counter.count           # 7
