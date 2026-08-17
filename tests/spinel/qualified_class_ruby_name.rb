# A nested class whose leaf name is a builtin's is qualified internally
# (`Brainfuck__Array`) so it cannot be mistaken for the builtin. Its
# Ruby-visible name is still `Brainfuck::Array`, through every spelling.

module Brainfuck
  class Array
    def self.bench_name
      name.to_s
    end
    def label
      self.class.name
    end
  end
end

p Brainfuck::Array.bench_name
p Brainfuck::Array.new.label
p Brainfuck::Array.name
