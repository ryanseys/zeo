# A `&block` parameter is real syntax now (a rejection this test
# used to check for was lifted).

class Foo
  def bar(&blk)
    blk.call(5)
  end
end
puts Foo.new.bar { |x| x * 2 }
__END__
10
