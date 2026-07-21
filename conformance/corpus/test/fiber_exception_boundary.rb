# #1474: a fiber's begin/rescue handlers must not leak across the fiber
# boundary. Each case previously crashed (cross-stack longjmp) or mis-routed.

# (b) fiber yields inside begin/rescue; resumer then raises with an outer handler
out = []
begin
  f = Fiber.new do
    begin
      out << "b:enter"
      Fiber.yield
    rescue => e
      out << "b:fiber-rescue(#{e.message})"
    end
  end
  f.resume
  out << "b:before-raise"
  raise "b-main"
rescue => e
  out << "b:OUTER(#{e.message})"
end
puts out.join(" ")

# (a) unhandled raise inside a fiber propagates to the resume site
begin
  g = Fiber.new { raise "a-fiber" }
  g.resume
rescue => e
  puts "a:caught(#{e.message})"
end

# (c) fiber yields mid-rescue; resumer enters its own begin; re-resume; fiber raises
log = []
h = Fiber.new do
  begin
    Fiber.yield
    raise "c-x"
  rescue => e
    log << "c:fiber-rescue(#{e.message})"
  end
end
h.resume
begin
  log << "c:main-begin"
rescue
end
h.resume
puts log.join(" ")
