class Foo
  def a
    "first"
  end

  def a
    "second"
  end
end

puts Foo.new.a
__END__
second
