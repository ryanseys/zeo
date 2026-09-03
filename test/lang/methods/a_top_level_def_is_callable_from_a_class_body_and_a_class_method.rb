# `self` in a class body/class method is the CLASS OBJECT -- there's no
# concrete receiver to clone, so implicit-self dispatch must go through
# `RubyValue::Class`, not an invalid `self.clone()`.

def helper(tag)
  "helped-#{tag}"
end
class AtBody
  RESULT = helper("body")
end
class Factory
  def self.build
    helper("class-method")
  end
end
puts AtBody::RESULT
puts Factory.build
__END__
helped-body
helped-class-method
