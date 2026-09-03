# An uncaught exception prints the message and the backtrace and exits 1. The
# linked binary carries the source names its backtrace needs.
def inner
  raise ArgumentError, "bad thing"
end

def outer
  inner
end

outer
__END__
#@ stderr
aot/uncaught_exception.rb:4:in 'Object#inner': bad thing (ArgumentError)
	from aot/uncaught_exception.rb:8:in 'Object#outer'
	from aot/uncaught_exception.rb:11:in '<main>'
#@ exit 1
