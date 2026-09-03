class Foo
  def initialize
    @tag = "foo"
  end

  def tag
    @tag
  end
end

class Bar
  def initialize
    @tag = "bar"
  end

  def tag
    @tag
  end
end

class Picker
  def pick(flag)
    if flag
      x = Foo.new
    else
      x = Bar.new
    end
    x
  end
end

picker = Picker.new
puts picker.pick(true).tag
puts picker.pick(false).tag
__END__
foo
bar
