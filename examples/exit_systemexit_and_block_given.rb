# `exit`/`abort` raise a RESCUABLE SystemExit carrying the status (it unwinds
# through ensure), and `block_given?` answers for an `&block` parameter through
# both the receiverless and the explicit `self.` spelling.
begin
  abort("bye now")
  puts "unreached"
rescue SystemExit => e
  puts "status=#{e.status}"
  puts "msg=#{e.message}"
end
puts "continued"

begin
  exit 3
rescue SystemExit => e
  puts "exit-status=#{e.status}"
  puts "exit-success=#{e.success?}"
end

# The unwind runs ensure blocks on its way out.
def guarded
  exit 0
ensure
  puts "ensure ran"
end
begin
  guarded
rescue SystemExit => e
  puts "guarded-status=#{e.status}"
end

def top(&block)
  block_given? ? "yes" : "no"
end
p top
p(top {})

def top_self(&block)
  self.block_given? ? "yes" : "no"
end
p top_self
p(top_self {})
puts "done"
