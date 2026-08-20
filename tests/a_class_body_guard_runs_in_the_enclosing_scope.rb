# A trailing `if`/`unless` on a `class` keyword belongs to the ENCLOSING
# scope, not to the class body: ruby evaluates it there (a raise inside one
# reports the enclosing frame, never `<class:X>`), and the locals it reads are
# the enclosing scope's. zeo lifts a class body to its own function, so the
# guard has to stay outside it.

on = true

class String
  def zeo_probe = "string probed"
end if on

p "x".zeo_probe

# `unless` puts the body in the other branch and runs when the guard is FALSE.
off = false

class Symbol
  def zeo_probe = "symbol probed"
end unless off

p :s.zeo_probe

# The guard is an ordinary expression, not just a bare read.
label = "yes"

class Integer
  def zeo_probe = "integer probed"
end if label == "yes"

p 1.zeo_probe

# A guard that does NOT pass defines nothing at all.
class NeverDefined
  def nope = 1
end if false
p defined?(NeverDefined)

module NeverAModule
  def nope = 1
end unless true
p defined?(NeverAModule)

# The guard runs in the ENCLOSING frame -- the label, not the line, is what
# both backends agree on with ruby here.
def boom = raise("guard raised")
begin
  class Float
    def zeo_probe = 1
  end if boom
rescue => e
  puts e.message
  puts e.backtrace.first(2).map { |l| l[/in '.*'/] }.inspect
end

# A class body's OWN `if` is not a guard and stays inside the body -- it
# reads what a class body can see, which is never an enclosing local.
FLAG = false
class Array
  if FLAG
    def zeo_probe = "never"
  else
    def zeo_probe = "array probed"
  end
end
p [].zeo_probe

# A class body's own local shadows the enclosing one rather than sharing it.
shadowed = "outer"
class Hash
  shadowed = "inner"
  def zeo_probe = "hash probed"
end if on
p shadowed
p({}.zeo_probe)
