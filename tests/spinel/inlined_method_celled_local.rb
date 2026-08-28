# A method local captured by a block is reached through a heap cell. When the
# method is INLINED at its call site -- which a `yield` in it forces -- the
# inlined body kept the cell form while the prologue declared a plain renamed
# local, and nothing declared the cell (#4088). Two halves: the cell form did
# not go through the rename map that the plain form did, and the three inline
# emitters each declared the plain form for a celled local.
#
# Same symptom as #4087 from the other side: there the block became a function
# of its own, here the callee is folded into the caller.

# ...and once it built, the repro answered 0 where CRuby answers 7: an inlined
# `each` over a poly receiver walks it AS A CONTAINER, and a user class that
# defines #each without #to_a -- the ordinary way to write one, forwarding the
# block -- walked as an empty array. emit_poly_iter_obj_normalize drives such a
# class's own #each with a collector now, which is what it already did for a
# class that answers #to_a.

class Bag
  def initialize(a) = @a = a
  def each(&b) = @a.each(&b)
end

class Yielder
  def initialize(a) = @a = a
  def each
    @a.each { |x| yield x }
  end
end

class WithToA
  def initialize(a) = @a = a
  def to_a = @a
  def each(&b) = @a.each(&b)
end

module M
  def self.sum_of(list)
    n = 0
    list.each { |x| n += yield x }
    n
  end
end

def run(list) = M.sum_of(list) { |x| x }
puts run([1, 2])
puts run([3, 4, 5])

# the cell types that are not sp_int
module F
  def self.join_of(list)
    s = ""
    list.each { |x| s += (yield x).to_s }
    s
  end

  def self.total_of(list)
    f = 0.0
    list.each { |x| f += (yield x) }
    f
  end

  def self.gather(list)
    a = []
    list.each { |x| a.push(yield x) }
    a
  end
end

def joined(list) = F.join_of(list) { |x| x }
def totalled(list) = F.total_of(list) { |x| x * 1.0 }
def gathered(list) = F.gather(list) { |x| x }
p joined([1, 2])
p totalled([1, 2])
p gathered([1, 2])

# a closure over the celled local outlives the loop
module K
  def self.keep(list)
    seen = []
    list.each { |x| seen.push(yield x) }
    seen.length
  end
end
def kept(list) = K.keep(list) { |x| x }
p kept([1, 2, 3])

p run(Bag.new([3, 4]))
p run(Yielder.new([5, 6]))
p run(WithToA.new([7, 8]))
p kept(Bag.new([1, 2, 3]))

# not inlined, which has always worked: the same answers
def walk(list)
  t = 0
  list.each { |x| t += x }
  t
end
p walk([1, 2])
p walk(Bag.new([3, 4]))
p walk(Yielder.new([5, 6]))

# a class with no each at all still walks as a container where it is one
p run([9])
