class Box
  def initialize(tag)
    @tag = tag
  end
  attr_reader :tag
end
def find_or_nil(want)
  if want == "yes"
    return Box.new("found")
  end
  nil
end
r = find_or_nil("yes")
puts r.send(:tag)
puts find_or_nil("no").inspect
__END__
found
nil
