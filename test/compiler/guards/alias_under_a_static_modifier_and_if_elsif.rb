# A statically-literal `if`/`unless` guard on an `alias` is folded at
# definition time: the selected branch's alias is registered.

class C
  def one = 1
  def two = 2
  alias uno one if true
  alias dos two unless false
  alias never one if false
  if false
    alias chosen one
  elsif true
    alias chosen two
  end
end
puts C.new.uno
puts C.new.dos
puts C.new.chosen
puts C.new.respond_to?(:never)
__END__
1
2
2
false
