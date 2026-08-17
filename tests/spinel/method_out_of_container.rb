class Calc
  def initialize(b); @b = b; end
  def add(n); @b + n; end
end
ms001 = [Calc.new(3).method(:add)]
p ms001[0].class
r002 = (ms001[0].name rescue $!.class); p r002
r003 = (ms001[0].owner rescue $!.class); p r003
r005 = (ms001[0].parameters rescue $!.class); p r005
h008 = { a: Calc.new(3).method(:add) }
r008 = (h008[:a].name rescue $!.class); p r008
