# The Integer lane of the operator fast-path suppression: a reopened
# Integer#== wins at literal sites and poly sites; + and include? keep
# their native paths.
class Integer
  def ==(other) = "int-eq(#{self},#{other})"
end
p 3 == 3
p 3 + 4
p [1, 2].include?(2)
y = [5].first
p y == 5
__END__
"int-eq(3,3)"
7
true
"int-eq(5,5)"
