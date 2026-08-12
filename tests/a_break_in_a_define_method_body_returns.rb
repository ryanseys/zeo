# `define_method(:go) { break :early }` answers `:early`: the block BECAME
# the method, so there is no yielding call left to break out of, and CRuby
# applies its lambda rule -- `break` returns from the method. rake's
# `define_method(:execute) { ... break if @failure ... }` (cxxproject) is
# the corpus shape. A real `def` cannot reach this: CRuby rejects a bare
# `break` in a method body outright.
class Runner
  def initialize(failed) = @failure = failed

  define_method(:execute) do |arg|
    break :bailed if @failure

    "ran #{arg}"
  end

  # A bare `break` with no value answers nil, like a bare `return`.
  define_method(:blank) { break }

  # ...and a `break` still breaks the INNER call when one is written between
  # it and the method body.
  define_method(:scan) do |items|
    found = nil
    items.each do |i|
      if i > 2
        found = i
        break
      end
    end
    break :none if found.nil?

    found
  end
end

p Runner.new(false).execute(:job)
p Runner.new(true).execute(:job)
p Runner.new(false).blank
p Runner.new(false).scan([1, 2, 5, 9])
p Runner.new(false).scan([1, 2])

# The singleton spelling takes the same rule.
class Gate
  define_singleton_method(:open?) { break false }
end
p Gate.open?
puts "still running"
