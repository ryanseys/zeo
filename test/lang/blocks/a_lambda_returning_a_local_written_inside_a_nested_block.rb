# A parser combinator whose inner each rebinds the outer local, which the
# lambda then answers.
def seq(parsers)
  ->(s) {
    rest = s
    parsers.each { |pp| out = pp.call(rest); rest = out[1] }
    rest
  }
end

lit = ->(str) { [str[0], str[1..]] }
r = seq([lit]).call("abc")
p r          # "bc"
p r.class    # String
__END__
"bc"
String
