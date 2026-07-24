# `$!` is the exception currently being handled -- inside any rescue,
# including the one-line modifier form, it refers to the caught exception,
# and is nil outside a rescue.
p $!                                    # nil (nothing being handled)

result = Integer("not a number") rescue $!.class
p result                                # ArgumentError

begin
  raise TypeError, "bad value"
rescue
  puts "handling: #{$!.message}"        # handling: bad value
  puts "class: #{$!.class}"             # class: TypeError
end

p $!                                    # nil again (handling finished)

# Exception#inspect renders "#<ClassName: message>", or just the class name
# when the message is empty.
p RuntimeError.new("boom").inspect      # "#<RuntimeError: boom>"
p ArgumentError.new("").inspect         # "ArgumentError"
