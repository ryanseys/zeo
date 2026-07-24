# nil.scan through a helper must compile (it emitted invalid C: an sp_RbVal
# element assigned into a const char* block-param slot) and raise NoMethodError
# at run time like CRuby, rather than failing C compilation.
def wrap(value)
  tokens = tokenize(value)
  tokens.each { |word| }
  :done
end

def tokenize(value) = value.scan(/\S+|\n/)

begin
  wrap(nil)
rescue NoMethodError
  puts "raised NoMethodError"
end
puts "ok"
