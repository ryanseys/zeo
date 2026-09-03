# The top-level report: innermost frame heads the message line, outer
# frames follow tab-indented -- verbatim ruby 4.0.6 (as `-e`).

def inner; raise "boom"; end
def outer; inner; end
outer
__END__
#@ stderr
lang/exceptions/uncaught_exception_report_matches_cruby_shape.rb:4:in 'Object#inner': boom (RuntimeError)
	from lang/exceptions/uncaught_exception_report_matches_cruby_shape.rb:5:in 'Object#outer'
	from lang/exceptions/uncaught_exception_report_matches_cruby_shape.rb:6:in '<main>'
#@ exit 1
