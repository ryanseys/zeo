module Loud
  def upcase = "<" + super + ">"
end

String.prepend(Loud)
p "ab".upcase
