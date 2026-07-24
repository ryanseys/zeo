# to_str ducks at String-argument sites.
class Sep
  def to_str
    "-"
  end
end

sep = Sep.new
p "a" + sep
p [1, 2, 3].join(sep)
p "a-b".split(sep)
p "a-b".index(sep)
p "x".center(5, sep)
p "a-b".start_with?(sep, "a")
p "b-".end_with?(sep)
s = +"a"
s << sep
p s
p "a-b-c".sub(sep, "+")
p "a-b".delete(sep)
p "a-b-c".count(sep)
