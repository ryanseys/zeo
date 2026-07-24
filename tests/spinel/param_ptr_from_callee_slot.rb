# A method param passed unmodified to a callee whose corresponding
# slot is `ptr` / a specific class pointer used to leave the caller's
# param typed `mrb_int`. The C compile then failed at the call site
# when the caller passed its mrb_int param into a `void *`-expecting
# slot.
#
# Mirrors the Db.column_bool shape in real-blog: a thin wrapper
# around a typed primitive (FFI sqlite handle) that didn't have its
# `stmt` param widened from the body's call to column_int.

module Box
  def self.read_int(p)
    p.length
  end

  def self.read_bool(p)
    read_int(p) != 0
  end
end

puts Box.read_int("abc")
puts Box.read_bool("abc")
puts Box.read_bool("")

# Same pattern across two layers — three-deep chain.
module Chain
  def self.head(s)
    middle(s)
  end

  def self.middle(s)
    tail(s)
  end

  def self.tail(s)
    s.length
  end
end

puts Chain.head("abc")
puts Chain.middle("xyz")
puts Chain.tail("hi")
