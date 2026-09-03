# The ec-swap's catch-tag slice, oracle-verified: a fiber has its own
# execution context, so the resumer's live `catch` frame is invisible
# inside it (UncaughtThrowError AT the throw) -- while a catch/throw
# pair fully inside the fiber works normally.

r = catch(:tag) do
  f = Fiber.new do
    begin
      throw :tag, 1
      :not_reached
    rescue UncaughtThrowError => e
      "uncaught: #{e.message}"
    end
  end
  f.resume
end
p r
f2 = Fiber.new { catch(:in) { throw :in, :works } }
p f2.resume
__END__
"uncaught: uncaught throw :tag"
:works
