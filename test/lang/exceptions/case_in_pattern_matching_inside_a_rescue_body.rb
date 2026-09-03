# Composes pattern matching with exception handling --
# `e.send(:message)` is a `Poly` expression, matched via `case/in`'s own
# `#deconstruct`-independent `ClassCheck`/`Capture` path.

begin
  raise "boom"
rescue => e
  case e.send(:message)
  in String => s
    puts "matched string: #{s}"
  end
end
__END__
matched string: boom
