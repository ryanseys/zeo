# webrick left ruby's bundled gems in 3.0, so the oracle ruby does not ship it
# and `require "webrick"` raises LoadError there too. zeo vendors under `gems/`
# only what the oracle ships -- vendoring webrick would make zeo answer a
# require ruby refuses, so this LoadError is the MATCHING answer, not a gap.
begin
  require "webrick"
rescue LoadError => e
  p e.class
  p e.message
  p e.path
end
p defined?(WEBrick)

# The same for the rest of the gems ruby 3.x retired.
%w[net-telnet sdbm].each do |name|
  begin
    require name
    p [name, :loaded]
  rescue LoadError => e
    p [name, e.message]
  end
end
