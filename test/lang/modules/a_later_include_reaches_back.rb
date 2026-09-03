module Extra
  def tag = :from_module
end

class Thing
end

p Thing.new.respond_to?(:tag)
p Thing.instance_methods.include?(:tag)

class Thing
  include Extra
end

p Thing.new.respond_to?(:tag)
p Thing.new.tag
__END__
false
false
true
:from_module
