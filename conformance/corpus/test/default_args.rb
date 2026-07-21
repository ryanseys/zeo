# Issue #49 (default args at call sites) and the scope follow-ups.
# Merged from default_arg_{ctor,method,inherited,partial,self_call,
# callee_scope}.rb. Each section covers a distinct dispatch path;
# the original split had only the dispatch path varying, so the
# class shapes mostly didn't collide. Where they did (`Foo` was
# reused in self_call and callee_scope for unrelated purposes) the
# names are made path-specific (SelfDefault / CalleeFoo).

# ---- Constructor default ----
# `Counter.new` (no arg) and `Counter.new(7)` both need to compile.
class Counter
  def initialize(start = 0)
    @n = start
  end
  attr_reader :n
end
puts Counter.new.n         # 0
puts Counter.new(7).n      # 7

# ---- Instance method default + inherited dispatch ----
# Same Greeter covers both: direct call and a subclass inheriting
# the defaulted method without redefining it.
class Greeter
  def hello(name = "world")
    puts "Hello, #{name}"
  end
end
class GreeterChild < Greeter
end
Greeter.new.hello                # Hello, world
Greeter.new.hello("Ruby")        # Hello, Ruby
GreeterChild.new.hello           # Hello, world
GreeterChild.new.hello("Ruby")   # Hello, Ruby

# ---- Partial defaulting ----
# Caller supplies some leading positional args; trailing defaults
# fill the rest.
class M
  def f(a, b = 1, c = 2)
    puts a + b + c
  end
end
m = M.new
m.f(10)             # 13
m.f(10, 20)         # 32
m.f(10, 20, 30)     # 60

# ---- Implicit-self call inside same class ----
# Bare-method dispatch from within a class body must also fill in
# the callee's defaults; previously this path called compile_call
# _args without the defaults map.
class SelfDefault
  def initialize
    bar
    bar(99)
  end
  def bar(x = 42)
    puts x
  end
end
SelfDefault.new
# 42
# 99

# ---- Default expression reads @ivar ----
# The inlined default expansion at the caller's site must resolve
# against the call's *receiver* (here Wrapper#grab's recv `@w`),
# not the caller's `self`. WrapperCaller has no @base, so a naive
# inlining would either fail to compile or read the wrong instance.
class Wrapper
  def initialize(seed)
    @base = seed
  end
  def grab(target = @base * 10)
    target
  end
end
class WrapperCaller
  def initialize
    @w = Wrapper.new(3)
  end
  def go
    @w.grab
  end
end
puts WrapperCaller.new.go        # 30

# ---- Default expression calls a same-class bare method ----
# CallerBar defines its own `target` returning 2, but CalleeFoo's
# default-arg `target` must still dispatch to CalleeFoo#target
# (returning 1) rather than the caller's vtable.
class CalleeFoo
  def target
    1
  end
  def foo(opt = target)
    opt
  end
end
class CallerBar
  def target
    2     # different value — must NOT be picked up by CalleeFoo#foo
  end
  def check
    CalleeFoo.new.foo
  end
end
puts CallerBar.new.check         # 1
