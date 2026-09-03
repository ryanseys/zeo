# Array/Hash#dig recurse through each intermediate's OWN #dig (CRuby's
# rb_obj_dig), so digging past a non-diggable (an Integer) raises
# TypeError rather than silently indexing its bits via Integer#[].

def cls; begin; yield; rescue => e; e.class; end; end
p(cls { [1, [2]].dig(1, 0, 3) })
p [1, [2]].dig(1, 0)
p [1, [2, [3]]].dig(1, 1, 0)
p [[nil]].dig(0, 0, 5)
p [{ a: 7 }].dig(0, :a)
p({ a: { b: 1 } }.dig(:a, :b))
p(cls { { a: 5 }.dig(:a, :b) })
__END__
TypeError
2
3
nil
7
1
TypeError
