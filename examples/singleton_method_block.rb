# A runtime-installed singleton method threads its call-site block, so
# yield / block_given? / &blk all work inside it (Batch G).

obj = Object.new

def obj.greet
  yield "world"
end
puts obj.greet { |x| "hello #{x}" }

def obj.wrap(&blk)
  blk.nil? ? "no block" : "[#{blk.call}]"
end
puts obj.wrap { "boxed" }
puts obj.wrap

class << obj
  def each_pair
    yield 1
    yield 2
    block_given? ? "done" : "noblock"
  end
end
seen = []
puts obj.each_pair { |n| seen << n }
p seen

# A `yield` reached with no block is a rescuable LocalJumpError, not a crash.
def obj.demands_block
  yield
end
begin
  obj.demands_block
rescue LocalJumpError => e
  puts "#{e.class}: #{e.message}"
end
