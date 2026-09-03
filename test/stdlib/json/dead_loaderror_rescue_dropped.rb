# A `begin; require "x"; rescue LoadError; <fallback def>` guard: when the
# require RESOLVES (a builtin/shim/file satisfied it), the rescue is DEAD --
# CRuby's behavior when the extension IS present -- so its fallback definitions
# never take effect. zeo drops the dead rescue at compile time (the shape
# rubygems' erb/strscan guards use).
begin
  require "json"
rescue LoadError
  class OnlyDefinedWhenJsonMissing
    def marker = "fallback"
  end
end

puts defined?(OnlyDefinedWhenJsonMissing).inspect
puts defined?(JSON).inspect
__END__
nil
"constant"
