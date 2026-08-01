# A class is an object, so it needs an `object_id` of its own. Every class
# used to report the same one, which quietly broke anything memoizing on it --
# a `seen[mod.object_id]` guard would treat the first class it met as every
# class it met afterwards.

MODS = [Object, BasicObject, Array, Hash, String, Symbol, Integer, Float,
        Comparable, Enumerable, Kernel, Math, Process, Struct, Class, Module]

puts "-- every class and module gets its own id"
ids = MODS.map(&:object_id)
p ids.uniq.size == MODS.size

puts "-- and the same one every time it is asked"
p MODS.map(&:object_id) == ids

puts "-- so a cache keyed on it distinguishes them"
seen = {}
MODS.each { |m| seen[m.object_id] = m }
p seen.size

puts "-- a class id never collides with an instance's"
objs = [Object.new, "s", [], {}, :sym, 1, 2.5, nil, true, false]
p (ids & objs.map(&:object_id)).empty?

puts "-- __id__ is the same answer"
p Array.__id__ == Array.object_id
p Array.object_id != Hash.object_id

puts "-- singleton classes are distinct too"
p Array.singleton_class.object_id != Hash.singleton_class.object_id
