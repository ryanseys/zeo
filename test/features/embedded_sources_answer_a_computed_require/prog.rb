name = ["greeter", ""].first
require name
p Greeter.hi("x")
p require(name)
require ["deep/nested", ""].first
p NESTED_OK
p $LOADED_FEATURES.grep(/greeter/)
