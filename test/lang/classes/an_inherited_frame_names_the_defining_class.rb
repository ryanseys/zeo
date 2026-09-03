# Ruby labels a backtrace frame with the class the `def` was WRITTEN in, not
# the class of the receiver it was called on. An inherited method called on a
# subclass still says `Base#boom`.
#
# zeo named the carrier -- `S1#boom` -- so every backtrace through an
# inherited method reported a class that never defined the method. The rule
# was already right for a module (`M#mixed`, never `Bar#mixed`) and the
# superclass half was simply excluded.
#
# The exception is a `def self.x` written inside `class << self`: its defining
# class is the singleton SURROGATE, an internal name no user can write, and
# ruby still spells that frame `Config.direct`. A surrogate is not in the
# owner's ancestry, which is what tells the two apart.

class Base
  def where = caller(0).first[/in '(.*)'/, 1]
  def boom = raise("from base")
end

module Mixed
  def mixed_where = caller(0).first[/in '(.*)'/, 1]
end

class Middle < Base
  include Mixed
end

class Leaf < Middle
  def own_where = caller(0).first[/in '(.*)'/, 1]
end

p [Base.new.where, Middle.new.where, Leaf.new.where]
p [Middle.new.mixed_where, Leaf.new.mixed_where]
p Leaf.new.own_where

# The same rule through a raise, three classes deep.
begin
  Leaf.new.boom
rescue RuntimeError => e
  puts e.backtrace.first
end

# A `def self.x` in a `class << self` body keeps naming the CLASS.
class Config
  class << self
    def direct = caller(0).first[/in '(.*)'/, 1]
  end
end
p Config.direct

# An overriding subclass names itself, because it is where the `def` is.
class Override < Base
  def boom = raise("from override")
end
begin
  Override.new.boom
rescue RuntimeError => e
  puts e.backtrace.first
end
__END__
["Base#where", "Base#where", "Base#where"]
["Mixed#mixed_where", "Mixed#mixed_where"]
"Leaf#own_where"
lang/classes/an_inherited_frame_names_the_defining_class.rb:17:in 'Base#boom'
"Config.direct"
lang/classes/an_inherited_frame_names_the_defining_class.rb:53:in 'Override#boom'
