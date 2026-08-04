# `const_missing` is never called for a constant read off a class the compiler
# can name: `ClsHost::Whatever` raises `NameError` directly.
#
# zeo resolves a qualified constant whose left side is a known class at COMPILE
# time, and a miss is baked as the raise. Ruby's miss is a call --
# `rb_const_missing` -- so the hook gets its chance at run time. The hook exists
# only to serve misses, so folding the miss removes the whole mechanism.
#
# The anonymous shape still works (the second half below), which is what makes
# this easy to miss: `Class.new { def self.const_missing(n) ... }` has no name
# for the compiler to fold against, so its reads stay dynamic and reach the
# hook. That is the reverse of what a programmer would predict.
#
# `const_missing` is how a loader materializes a class on first reference --
# every autoloading framework is built on it -- and those all define constants
# with real names.

class ClsHost
  def self.const_missing(name) = [:cls_missing, name]
end
module ModHost
  def self.const_missing(name) = [:mod_missing, name]
end
module ModHost2
  class << self
    def const_missing(name) = [:mod2_missing, name]
  end
end

p ClsHost::Whatever
p ModHost::Whatever
p ModHost2::Whatever
p ClsHost.const_get(:Whatever)
p ModHost.const_get(:Whatever)

# The anonymous form already reaches the hook.
anon = Class.new { def self.const_missing(name) = [:anon_missing, name] }
p anon.const_get(:Nope)
