# CRuby: the fiber has its own errinfo (fresh, nil), so its bare
# `raise` builds a fresh empty-message RuntimeError instead of
# re-raising the resumer's in-flight exception -- `fiber saw: []`.

f = Fiber.new do
  begin
    raise
  rescue RuntimeError => e
    "fiber saw: [#{e.send(:message)}]"
  end
end
begin
  raise "resumer's exception"
rescue RuntimeError
  puts f.resume
end
__END__
fiber saw: []
