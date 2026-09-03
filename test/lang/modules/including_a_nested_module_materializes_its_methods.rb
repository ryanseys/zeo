module Util
  module Greet
    def hello
      "hello from nested module"
    end
  end
end

class Greeter2
  include Util::Greet
end

puts Greeter2.new.hello
__END__
hello from nested module
