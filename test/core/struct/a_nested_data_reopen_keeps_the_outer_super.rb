# A Data reopen whose body holds ANOTHER Data class with its own
# `initialize`/`super`. The nested `class Particle` is what pushed the outer
# reopen onto the static path, and the outer `super` then reached
# `Object#initialize`: "wrong number of arguments (given 1, expected 0)".
# Sow's `lib/sow/particles.rb` is exactly this shape.

module Sow
  Particles = Data.define(:max, :live)
  class Particles
    def initialize(max: 256, live: [])
      super(max: max, live: live.freeze)
    end

    Particle = Data.define(:x, :age)
    class Particle
      def initialize(x:, age: 0.0)
        super(x: x.to_f, age: age.to_f)
      end
    end
  end
end
p Sow::Particles.new(max: 320)
p Sow::Particles.new.live.frozen?
p Sow::Particles::Particle.new(x: 1)
p Sow::Particles::Particle.new(x: 1, age: 2).age
__END__
#<data Sow::Particles max=320, live=[]>
true
#<data Sow::Particles::Particle x=1.0, age=0.0>
2.0
