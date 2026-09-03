# The optional-dependency idiom: `begin; require 'missing'; rescue
# LoadError`. A literal require that can't be resolved but is lexically
# inside a `rescue LoadError` is deferred to a runtime `Kernel#require`
# (raising the LoadError the rescue handles), not a loud compile error.
# net/http's `begin; require 'win32/sspi'; rescue LoadError` needs this.

begin
  require "definitely_missing_optdep_xyz"
  puts "loaded"
rescue LoadError => e
  puts "rescued: #{e.send(:message)}"
end
puts "after"
__END__
rescued: cannot load such file -- definitely_missing_optdep_xyz
after
