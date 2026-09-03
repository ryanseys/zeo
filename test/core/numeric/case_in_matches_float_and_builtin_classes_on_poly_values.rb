def describe(val)
  case val
  in Integer then "int"
  in Float then "float"
  in String then "str"
  else "other"
  end
end
puts describe(1)
puts describe(2.5)
puts describe("s")
puts describe(:sym)
__END__
int
float
str
other
