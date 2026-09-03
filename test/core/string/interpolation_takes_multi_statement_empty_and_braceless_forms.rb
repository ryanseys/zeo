p "v=#{1; 2}"
p "v=#{a = 3; a * 2}"
p a
p "x#{}y"
$g = "glob"
class C
  def initialize; @iv = "ivar"; end
  def show; "iv=#@iv g=#$g"; end
end
p C.new.show
__END__
"v=2"
"v=6"
3
"xy"
"iv=ivar g=glob"
