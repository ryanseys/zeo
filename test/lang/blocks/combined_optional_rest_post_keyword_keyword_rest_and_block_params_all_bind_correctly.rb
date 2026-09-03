class Widget
  def build(a, b = 10, *rest, c, d: 5, **kw, &blk)
    puts a
    puts b
    puts rest
    puts c
    puts d
    puts kw[:e]
    puts blk.call
  end
end
Widget.new.build(1, 2, 3, 4, 5, d: 99, e: 100) { "block!" }
__END__
1
2
3
4
5
99
100
block!
