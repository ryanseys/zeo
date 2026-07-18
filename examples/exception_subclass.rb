# A user exception subclass is backed by the native RubyException (D3): custom
# ivars, `super` into the native initialize (hidden message slot), multi-level
# inheritance, class methods, and rescue-by-ancestry all match CRuby.

class AppError < StandardError
  def initialize(code)
    @code = code
    super("app error #{code}")
  end

  def code
    @code
  end

  def self.with_code(n)
    new(n)
  end
end

class NotFound < AppError
  def initialize
    super(404)
    @where = "here"
  end

  def where
    @where
  end
end

e = AppError.new(7)
puts e.message
puts e.code
p e.instance_variables
p e.instance_variable_get(:@message)

nf = NotFound.new
puts nf.message
puts nf.code
puts nf.where
puts nf.is_a?(AppError)
puts nf.is_a?(StandardError)

puts AppError.with_code(9).message

begin
  raise NotFound
rescue AppError => ex
  puts "rescued #{ex.class}: #{ex.message}"
end

# A subclass with no initialize inherits the native default.
class Plain < RuntimeError
end
puts Plain.new("plain msg").message
p Plain.new("plain msg").instance_variables
