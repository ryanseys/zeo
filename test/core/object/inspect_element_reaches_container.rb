# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A container holding itself, which is what the golden inspects.
#@ gccheck: cycle leak: 2 objects (Array x1, Hash x1)
# An element's `inspect`/`to_s` may read the very container being printed.
#
# The `seen` stack guards a container that contains ITSELF (`a << a` prints
# `[...]`), but it cannot guard this: the recursion leaves through a USER
# method, which reaches the container by another name entirely. Rendering
# therefore must not hold the payload lock across the element dispatch --
# a non-reentrant Mutex turns that into a silent hang, with no output and
# no error, which is how it escaped notice.
class Peeker
  def inspect
    "<#{$arr.length}>"
  end

  def to_s
    "<s#{$arr.length}>"
  end
end

$arr = [Peeker.new, 2]
p $arr
puts $arr.to_s
puts "#{$arr}"

class HashPeeker
  def inspect
    "<#{$h.size}>"
  end
end

$h = { k: HashPeeker.new, j: 2 }
p $h
puts $h.to_s

# The element reaches a DIFFERENT container that in turn holds the first,
# so the two renderings interleave without either being self-referential.
class CrossPeeker
  def inspect
    "<#{$other.length}>"
  end
end

$other = [1, 2, 3]
p [CrossPeeker.new, $other]

# Still prints the cycle marker when the container really does contain
# itself -- the snapshot must not cost us that.
cyc = [1]
cyc << cyc
p cyc

hcyc = {}
hcyc[:self] = hcyc
p hcyc
__END__
[<2>, 2]
[<2>, 2]
[<2>, 2]
{k: <2>, j: 2}
{k: <2>, j: 2}
[<3>, [1, 2, 3]]
[1, [...]]
{self: {...}}
