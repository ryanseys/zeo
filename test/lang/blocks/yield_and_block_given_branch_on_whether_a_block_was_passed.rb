class Foo
  def maybe_yield
    if block_given?
      yield 42
    else
      -1
    end
  end
end
f = Foo.new
puts f.maybe_yield { |x| x * 2 }
puts f.maybe_yield
__END__
84
-1
