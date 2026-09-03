obj = Object.new
def obj.greet
  yield "world"
end
puts obj.greet { |x| "hello #{x}" }
def obj.maybe
  block_given? ? yield(5) : "none"
end
puts obj.maybe
puts obj.maybe { |n| n * 100 }
__END__
hello world
none
500
