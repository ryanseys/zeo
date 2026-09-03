# The ec-swap's home-stack slice, oracle-verified: a method running
# inside a fiber suspends mid-body, and after resumption its
# return-proc still unwinds to it (the home stayed alive across the
# suspension because the whole stack was parked, not shared); a proc
# that ESCAPES its dead activation still gets the LocalJumpError.

def maker
  pr = proc { return :via_proc }
  Fiber.yield :suspended
  pr.call
  :after_call
end
f = Fiber.new { maker }
p f.resume
p f.resume
def escape_maker
  proc { return :late }
end
f2 = Fiber.new do
  pr = escape_maker
  begin
    pr.call
  rescue LocalJumpError => e
    "LJE: #{e.message}"
  end
end
p f2.resume
__END__
:suspended
:via_proc
"LJE: unexpected return"
