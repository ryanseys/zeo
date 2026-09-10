# Kernel conversions do not consult the to_int/to_str/to_ary protocol
# methods.
#
class ToStr001; def to_str; "c2str"; end; end
p(String(ToStr001.new) == "c2str")

class ToA002; def to_a; [1, 2]; end; end
p(Array(ToA002.new) == [1, 2])
p(Array(ToA002.new).size)

class ToAry003; def to_ary; [3, 4]; end; end
p(Array(ToAry003.new) == [3, 4])

class ToHash004; def to_hash; { z: 9 }; end; end
r004 = (Hash(ToHash004.new) rescue $!.class); p r004

class ToS005; def to_s; "c1"; end; end
p(String(ToS005.new))
__END__
true
true
2
true
{z: 9}
"c1"
