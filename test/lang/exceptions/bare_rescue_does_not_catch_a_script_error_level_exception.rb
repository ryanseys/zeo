# A bare `rescue` matches `StandardError` and below ONLY -- it must NOT
# catch a raised `ScriptError` (a sibling branch of the hierarchy, both
# direct children of `Exception`). This is the real semantic bare
# `rescue`'s default narrows to `StandardError`, not `Exception`.

begin
  begin
    raise ScriptError, "script problem"
  rescue => e
    puts "should not print: #{e.send(:message)}"
  end
rescue ScriptError => e
  puts "outer caught ScriptError: #{e.send(:message)}"
end
__END__
outer caught ScriptError: script problem
