# A `&blk` param on a per-object singleton binds to the method's call-site
# block (nil when none is passed), not the unconditional nil it used to.

obj = Object.new
def obj.wrap(&blk)
  blk.nil? ? "no block" : blk.call(41) + 1
end
puts obj.wrap { |n| n }
puts obj.wrap
__END__
42
no block
