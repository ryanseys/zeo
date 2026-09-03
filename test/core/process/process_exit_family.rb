# Process.exit / Process.abort are the module-function twins of the Kernel
# forms: both raise a RESCUABLE SystemExit carrying the status, unwinding
# through ensure, and abort writes its message to stderr first.
begin
  Process.exit(5)
  puts "unreached"
rescue SystemExit => e
  puts "exit-status=#{e.status}"
  puts "exit-success=#{e.success?}"
end
puts "continued"

begin
  Process.abort("boom-message")
rescue SystemExit => e
  puts "abort-status=#{e.status}"
  puts "abort-msg=#{e.message}"
end

# The unwind runs ensure blocks on its way out of Process.exit too.
def guarded
  Process.exit(0)
ensure
  puts "ensure ran"
end
begin
  guarded
rescue SystemExit => e
  puts "guarded-status=#{e.status}"
end
puts "done"
__END__
exit-status=5
exit-success=false
continued
abort-status=1
abort-msg=boom-message
ensure ran
guarded-status=0
done
#@ stderr
boom-message
