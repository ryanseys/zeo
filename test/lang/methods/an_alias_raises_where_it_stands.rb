# An alias whose source resolves nowhere raises NameError where the
# statement stands: the class body before it has run, code after it has
# not, and a rescue around it catches it. `alias_method` raises from its own
# C frame. A top-level alias raises at its own line, after the output
# before it.
$stdout.sync = true
puts "before"
class A
  begin
    alias x nope_a
  rescue NameError => e
    puts "rescued in body: #{e.message}"
  end
  puts "after in body"
end
begin
  class B
    alias_method :y, :nope_b
    puts "unreached in B"
  end
rescue NameError => e
  puts "rescued outside: #{e.message} #{e.backtrace.first}"
end
class C
  alias_method :to_str2, :to_s
end
p C.new.respond_to?(:to_str2)
puts "top alias next"
alias top_nope missing_top_method
puts "unreached"
__END__
before
rescued in body: undefined method 'nope_a' for class 'A'
after in body
rescued outside: undefined method 'nope_b' for class 'B' lang/methods/an_alias_raises_where_it_stands.rb:18:in 'Module#alias_method'
true
top alias next
#@ stderr
lang/methods/an_alias_raises_where_it_stands.rb:29:in '<main>': undefined method 'missing_top_method' for class 'Object' (NameError)
#@ exit 1
