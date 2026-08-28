class Aliased
  def self.original = __callee__
  singleton_class.alias_method :nickname, :original
end

puts "original #{Aliased.original.inspect}"
puts "alias    #{Aliased.nickname.inspect}"
