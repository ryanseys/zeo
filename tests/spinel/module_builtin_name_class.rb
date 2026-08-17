module M
  class Bench
    def self.bench_name; "base"; end
    def name_of; self.class.bench_name; end
  end
  class Array < Bench
    def self.bench_name; "arr"; end
    def hi; "hello"; end
  end
  class Hash < Bench
    def self.bench_name; "hsh"; end
  end
end
p M::Array.new.hi
p M::Array.new.name_of
p M::Hash.new.name_of
p M::Array.bench_name
a = [1, 2]
p a.class.to_s
p({ x: 1 }.class.to_s)
