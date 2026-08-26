module Demo
  class Spec
    def self.value = Thing::VALUE
  end
  # A METHOD-BODY require is lazy, so `cmd.rb` becomes a unit -- and
  # `thing.rb` is pulled into that unit rather than into the program.
  def self.late
    require_relative "cmd"
  end
end
