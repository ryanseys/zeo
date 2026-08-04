# An @ivar holding a table of int arrays gets the typed nested representation
# (an sp_PtrArray of sp_IntArray*), but only when EVERY reference to it is an
# op the narrowing models, from the owning class itself. An ivar's references
# span the whole class and escape through readers, returns and arguments, and a
# missed one is not a lost optimization -- it is an sp_PtrArray * meeting
# sp_RbVal in the emitted C. Each class below is one way out; only the last
# modeled one may narrow, and all of them must answer as CRuby does.
class A1
  attr_reader :t
  def initialize(n) = @t = Array.new(n) { Array.new(n, 0) }
end
p A1.new(2).t.map { |r| r.length }

# 2. the table returned bare from a method
class A2
  def initialize(n) = @t = Array.new(n) { Array.new(n, 0) }
  def rows = @t
end
p A2.new(2).rows.length

# 3. the table passed to another method
class A3
  def initialize(n) = @t = Array.new(n) { Array.new(n, 0) }
  def go = count(@t)
  def count(x) = x.length
end
p A3.new(3).go

# 4. an unmodeled op on the table
class A4
  def initialize(n) = @t = Array.new(n) { Array.new(n, 0) }
  def go
    s = 0
    @t.each { |r| s += r.length }
    s
  end
end
p A4.new(3).go

# 5. a subclass reading the parent's table
class A5
  def initialize(n) = @t = Array.new(n) { Array.new(n, 0) }
end
class A5b < A5
  def peek = @t[0][0]
end
p A5b.new(2).peek

# 6. the modeled shape: should narrow
class A6
  def initialize(n)
    @n = n
    @t = Array.new(n) { Array.new(n, 0) }
  end
  def bump
    i = 0
    while i < @n
      @t[i][i] = @t[i][i] + i
      i += 1
    end
    self
  end
  def sum
    s = 0
    r = 0
    while r < @n
      row = @t[r]
      c = 0
      while c < @n
        s += row[c]
        c += 1
      end
      r += 1
    end
    s
  end
end
p A6.new(5).bump.sum

# 7. mixed element types: not an int table
class A7
  def initialize(n) = @t = Array.new(n) { ["a", 1] }
  def go = @t[0][0]
end
p A7.new(2).go

# 8. an array LITERAL of int arrays, the shape optcarrot's @nmt_mem has: the
# ivar write needs its own pointer-array form, and without it the poly array it
# built landed in an sp_PtrArray * field and the emulator core-dumped.
class A8
  def initialize
    @m = [[1, 2], [3, 4]]
  end
  def go
    @m[0][1] = @m[0][1] + 10
    @m[0][1] + @m[1][0] + @m.length
  end
end
p A8.new.go
