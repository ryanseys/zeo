# A nested block reading the ENCLOSING block's parameter, where the inner call
# is one a user class owns and the block is therefore materialized as a proc.
#
# An inlined iterator block's parameter binds in the loop, so there is no cell
# for a proc to close over. A proc LITERAL there is already handled: the body
# is wrapped in an immediately-called lambda that owns the variable, and the
# enclosing-proc-param capture machinery takes it from there. A block lifted
# because a user class owns the name was not recognised as such a proc, so the
# wrapper never fired and codegen met a capture with no storage.
#
# Two things behind it: only the BLOCK of such a call is lifted -- its receiver
# and arguments are evaluated in the enclosing frame, so a name read there is
# not a capture -- and a proc built INSIDE another proc has to forward the cell
# from that proc's capture struct, since no `_cell_<name>` is in scope there.

class Rel
  def each
    yield "row"
  end
end

class R
  def conditions(hash)
    out = ""
    hash.each do |key, val|
      val.each { |col| out = "#{key}.#{col}" }
    end
    out
  end

  # both the enclosing block's param and a method local, accumulated
  def pairs(hash)
    acc = []
    hash.each do |key, val|
      val.each { |col| acc << "#{key}=#{col}" }
    end
    acc
  end

  # the outer param read from the inner call's RECEIVER, which is evaluated in
  # the enclosing frame and is not a capture
  def firsts(rows)
    out = []
    rows.each do |row|
      row.first.each { |ch| out << ch }
    end
    out
  end

  # three levels
  def deep(hash)
    out = []
    hash.each do |key, vals|
      vals.each do |val|
        [1, 2].each { |n| out << "#{key}#{val}#{n}" }
      end
    end
    out
  end
end

r = R.new
p r.conditions({"t" => ["c"]})
p r.pairs({"a" => ["x", "y"], "b" => ["z"]})
p r.firsts([[["p", "q"]], [["s"]]])
p r.deep({"k" => ["v"]})

# the user arm is still reached
p Rel.new.each { |x| x }

# and a plain proc literal capturing an inlined block param still works
res = []
{"h" => 1}.each do |k, v|
  f = ->(n) { "#{k}:#{n}" }
  res << f.call(v)
end
p res
